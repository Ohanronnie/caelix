import { CustomTransportStrategy, Server } from "@nestjs/microservices";
import { connect, JSONCodec, NatsConnection, Subscription } from "nats";
import { isObservable, lastValueFrom } from "rxjs";

const protocolVersion = 1;
const defaultMaximumRequestBytes = 1024 * 1024;

interface RequestEnvelope {
  version: number;
  correlation_id: string;
  deadline_unix_millis: number | null;
  headers: Record<string, string>;
  payload: unknown;
}

interface RemoteError {
  code: string;
  message: string;
  details: unknown | null;
  retryable: boolean;
}

interface ResponseEnvelope {
  version: number;
  correlation_id: string;
  body:
    | { kind: "success"; payload: unknown }
    | {
        kind: "error";
        code: string;
        message: string;
        details: unknown | null;
        retryable: boolean;
      };
}

/**
 * Nest custom transport which uses Caelix's public Core NATS request envelope.
 * Nest's built-in NATS transport intentionally has a different packet shape,
 * so cross-framework request/reply requires this explicit adapter.
 */
export class CaelixNatsServer
  extends Server
  implements CustomTransportStrategy
{
  private connection?: NatsConnection;
  private readonly subscriptions: Subscription[] = [];
  private readonly consumers = new Set<Promise<void>>();
  private readonly codec = JSONCodec<unknown>();

  constructor(
    private readonly server: string,
    private readonly queue: string,
    private readonly maximumRequestBytes = defaultMaximumRequestBytes,
  ) {
    super();
  }

  async listen(callback: () => void): Promise<void> {
    try {
      this.connection = await connect({ servers: this.server });

      for (const [pattern, registeredHandler] of this.messageHandlers) {
        const handler = registeredHandler as Function & {
          isEventHandler?: boolean;
        };
        if (handler.isEventHandler) continue;
        const subscription = this.connection.subscribe(pattern, {
          queue: this.queue,
        });
        this.subscriptions.push(subscription);
        let consumer: Promise<void>;
        consumer = this.consume(subscription, handler).finally(() => {
          this.consumers.delete(consumer);
        });
        this.consumers.add(consumer);
      }

      await this.connection.flush();
      callback();
    } catch (error) {
      await this.close();
      throw error;
    }
  }

  async close(): Promise<void> {
    this.subscriptions.forEach((subscription) => subscription.unsubscribe());
    await Promise.race([
      Promise.allSettled(this.consumers),
      new Promise<void>((resolve) => setTimeout(resolve, 10_000)),
    ]);
    await this.connection?.drain();
  }

  on(_event: string, _callback: Function): never {
    throw new Error("CaelixNatsServer does not expose transport events");
  }

  unwrap<T = never>(): T {
    return this.connection as T;
  }

  private async consume(
    subscription: Subscription,
    handler: Function,
  ): Promise<void> {
    for await (const message of subscription) {
      if (!message.reply) continue;
      try {
        const request = this.decodeRequest(message.data);
        const response = await this.dispatch(request, handler);
        this.connection?.publish(message.reply, this.codec.encode(response));
      } catch {
        // A malformed request must not terminate this long-lived subscription.
        // It has no trustworthy correlation ID, so there is no protocol-safe reply.
      }
    }
  }

  private decodeRequest(data: Uint8Array): RequestEnvelope {
    if (data.byteLength > this.maximumRequestBytes) {
      throw new Error("Caelix request envelope exceeds the configured maximum");
    }
    const request = this.codec.decode(data) as RequestEnvelope;
    if (request.version !== protocolVersion || !request.correlation_id) {
      throw new Error("unsupported Caelix request envelope");
    }
    return request;
  }

  private async dispatch(
    request: RequestEnvelope,
    handler: Function,
  ): Promise<ResponseEnvelope> {
    if (
      request.deadline_unix_millis !== null &&
      request.deadline_unix_millis < Date.now()
    ) {
      return {
        version: protocolVersion,
        correlation_id: request.correlation_id,
        body: {
          kind: "error",
          code: "Deadline Exceeded",
          message: "request deadline elapsed before handler invocation",
          details: null,
          retryable: false,
        },
      };
    }
    try {
      const value = await handler(request.payload);
      const payload =
        (isObservable(value) ? await lastValueFrom(value) : value) ?? null;
      return {
        version: protocolVersion,
        correlation_id: request.correlation_id,
        body: { kind: "success", payload },
      };
    } catch (error) {
      const remote: RemoteError = {
        code: "Internal Server Error",
        message: "Internal Server Error",
        details: null,
        retryable: true,
      };
      return {
        version: protocolVersion,
        correlation_id: request.correlation_id,
        body: { kind: "error", ...remote },
      };
    }
  }
}
