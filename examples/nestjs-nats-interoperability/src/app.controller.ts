import { Controller } from "@nestjs/common";
import { MessagePattern, Payload } from "@nestjs/microservices";

interface EchoRequest {
  value: string;
}

@Controller()
export class AppController {
  @MessagePattern("interop.echo")
  echo(@Payload() request: EchoRequest): EchoRequest {
    return { value: `nest:${request.value}` };
  }
}
