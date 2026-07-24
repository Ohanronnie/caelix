import "reflect-metadata";
import { NestFactory } from "@nestjs/core";
import { AppModule } from "./app.module.js";
import { CaelixNatsServer } from "./caelix-nats.strategy.js";

const app = await NestFactory.createMicroservice(AppModule, {
  strategy: new CaelixNatsServer(
    process.env.NATS_URL ?? "nats://127.0.0.1:4222",
    "nestjs-interop-service",
  ),
});

app.enableShutdownHooks();
await app.listen();
