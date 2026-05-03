export class EventBus {
  #handlers = new Map();
  #events = [];

  on(type, handler) {
    const handlers = this.#handlers.get(type) ?? [];
    handlers.push(handler);
    this.#handlers.set(type, handlers);
    return () => this.off(type, handler);
  }

  off(type, handler) {
    const handlers = this.#handlers.get(type) ?? [];
    this.#handlers.set(type, handlers.filter((candidate) => candidate !== handler));
  }

  async emit(type, payload) {
    const event = { type, payload, emittedAt: new Date().toISOString() };
    this.#events.push(event);
    for (const handler of this.#handlers.get(type) ?? []) {
      await handler(event);
    }
    return event;
  }

  history() {
    return [...this.#events];
  }
}
