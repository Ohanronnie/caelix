const assert = require("node:assert/strict");
const { io } = require("socket.io-client");

const port = process.argv[2];
if (!port) throw new Error("expected server port");

const connect = (namespace) => new Promise((resolve, reject) => {
  const socket = io(`http://127.0.0.1:${port}${namespace}`, {
    transports: ["websocket"],
    timeout: 5000,
  });
  socket.once("connect", () => resolve(socket));
  socket.once("connect_error", reject);
});

const emitAck = (socket, event, payload) => new Promise((resolve, reject) => {
  socket.timeout(3000).emit(event, payload, (error, reply) => {
    if (error) reject(error);
    else resolve(reply);
  });
});

(async () => {
  const socket = await connect("/chat");
  const roomMember = await connect("/chat");
  const outsideRoom = await connect("/chat");
  const otherNamespace = await connect("/other");
  const closeAll = () => [socket, roomMember, outsideRoom, otherNamespace].forEach((client) => client.close());
  try {
    assert.equal(await emitAck(socket, "echo", "hello"), "hello");
    assert.equal(await emitAck(otherNamespace, "echo", "hello"), "other:hello");

    let roomMessage;
    let outsideReceived = false;
    roomMember.once("room-message", (message) => { roomMessage = message; });
    outsideRoom.once("room-message", () => { outsideReceived = true; });
    assert.equal(await emitAck(socket, "join", "room"), "joined");
    assert.equal(await emitAck(roomMember, "join", "room"), "joined");
    assert.equal(await emitAck(socket, "announce", "hello room"), "sent");
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.equal(roomMessage, "hello room");
    assert.equal(outsideReceived, false);

    let errorEvent;
    socket.once("error", (payload) => { errorEvent = payload; });
    const replyError = await emitAck(socket, "fail", "ignored");
    assert.equal(replyError.error, "Bad Request");
    assert.equal(replyError.message, "bad input");
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.deepEqual(errorEvent, replyError);
    closeAll();
  } catch (error) {
    closeAll();
    throw error;
  }
})().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
