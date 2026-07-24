import "reflect-metadata";
import { NestFactory } from "@nestjs/core";
import { AppModule } from "./app.module.js";
import { CaelixRedisServer } from "./caelix-redis.strategy.js";

const redisUrl = process.env.CAELIX_REDIS_URL ?? "redis://127.0.0.1:6379/";
const app = await NestFactory.createMicroservice(AppModule, {
  strategy: new CaelixRedisServer(redisUrl, "nestjs-interop"),
});
await app.listen();
console.log("NestJS Caelix Redis transport is ready");
