import { createHash, randomUUID } from "node:crypto";
import { CustomTransportStrategy, Server } from "@nestjs/microservices";
import { Redis } from "ioredis";
import { isObservable, lastValueFrom } from "rxjs";

const protocolVersion = 1;

interface RequestEnvelope {
  version: number;
  correlation_id: string;
  deadline_unix_millis: number | null;
  reply_channel: string;
  headers: Record<string, string>;
  payload: unknown;
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
        details: null;
        retryable: boolean;
      };
}

const stableName = (value: string): string =>
  createHash("sha256").update(value).digest("hex").slice(0, 24);
const commandStream = (subject: string): string =>
  `caelix:commands:${stableName(subject)}`;
const commandGroup = (service: string, subject: string): string =>
  `caelix-command-${stableName(service)}-${stableName(subject)}`;

export class CaelixRedisServer
  extends Server
  implements CustomTransportStrategy
{
  private readonly commands: Redis;
  private readonly subscribers: Redis[] = [];
  private readonly consumers = new Set<Promise<void>>();
  private stopped = false;

  constructor(
    redisUrl: string,
    private readonly serviceName: string,
    private readonly maximumRequestBytes = 1024 * 1024,
  ) {
    super();
    this.commands = new Redis(redisUrl, { lazyConnect: true });
  }

  async listen(callback: () => void): Promise<void> {
    await this.commands.connect();
    for (const [pattern, registeredHandler] of this.messageHandlers) {
      const handler = registeredHandler as Function & {
        isEventHandler?: boolean;
      };
      if (handler.isEventHandler) continue;
      const stream = commandStream(pattern);
      const group = commandGroup(this.serviceName, pattern);
      try {
        await this.commands.xgroup("CREATE", stream, group, "0", "MKSTREAM");
      } catch (error) {
        if (!(error instanceof Error) || !error.message.includes("BUSYGROUP"))
          throw error;
      }
      const subscriber = this.commands.duplicate();
      this.subscribers.push(subscriber);
      await subscriber.connect();
      let consumer: Promise<void>;
      consumer = this.consume(subscriber, stream, group, handler).finally(
        () => {
          this.consumers.delete(consumer);
        },
      );
      this.consumers.add(consumer);
    }
    callback();
  }

  async close(): Promise<void> {
    this.stopped = true;
    await Promise.allSettled(
      this.subscribers.map((subscriber) => subscriber.quit()),
    );
    await Promise.allSettled(this.consumers);
    await this.commands.quit();
  }

  on(_event: string, _callback: Function): never {
    throw new Error("CaelixRedisServer does not expose transport events");
  }

  unwrap<T = never>(): T {
    return this.commands as T;
  }

  private async consume(
    redis: Redis,
    stream: string,
    group: string,
    handler: Function,
  ): Promise<void> {
    const consumer = randomUUID();
    while (!this.stopped) {
      const response = (await redis.xreadgroup(
        "GROUP",
        group,
        consumer,
        "COUNT",
        1,
        "BLOCK",
        500,
        "STREAMS",
        stream,
        ">",
      )) as [string, [string, string[]][]][] | null;
      if (!response) continue;
      for (const [, entries] of response) {
        for (const [id, fields] of entries) {
          await this.process(stream, group, id, fields, handler);
        }
      }
    }
  }

  private async process(
    stream: string,
    group: string,
    id: string,
    fields: string[],
    handler: Function,
  ): Promise<void> {
    try {
      const envelopeIndex = fields.indexOf("envelope");
      if (envelopeIndex < 0) return;
      const encoded = fields[envelopeIndex + 1];
      if (Buffer.byteLength(encoded) > this.maximumRequestBytes) return;
      const request = JSON.parse(encoded) as RequestEnvelope;
      if (
        request.version !== protocolVersion ||
        !request.correlation_id ||
        !request.reply_channel
      )
        return;
      if (
        request.deadline_unix_millis !== null &&
        request.deadline_unix_millis < Date.now()
      )
        return;
      const response = await this.dispatch(request, handler);
      await this.commands.publish(
        request.reply_channel,
        JSON.stringify(response),
      );
    } finally {
      await this.commands.xack(stream, group, id);
      await this.commands.xdel(stream, id);
    }
  }

  private async dispatch(
    request: RequestEnvelope,
    handler: Function,
  ): Promise<ResponseEnvelope> {
    try {
      const result = await handler(request.payload);
      const payload =
        (isObservable(result) ? await lastValueFrom(result) : result) ?? null;
      return {
        version: protocolVersion,
        correlation_id: request.correlation_id,
        body: { kind: "success", payload },
      };
    } catch {
      return {
        version: protocolVersion,
        correlation_id: request.correlation_id,
        body: {
          kind: "error",
          code: "Internal Server Error",
          message: "Internal Server Error",
          details: null,
          retryable: true,
        },
      };
    }
  }
}
