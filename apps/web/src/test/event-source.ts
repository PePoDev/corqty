// ponytail: a test-only EventSource over fetch. jsdom has none, and Node's
// (behind --experimental-eventsource) uses undici internals that MSW never
// sees. msw/sse needs a global EventSource and serves the stream through its
// fetch interceptor, so this one is enough. It follows the WHATWG processing
// model for what the app relies on: open, unnamed and named events, `id:` ->
// lastEventId, `retry:`, Last-Event-ID on reconnect, fail (CLOSED, no retry)
// on a non-200 or non-event-stream response, reconnect when the body ends or
// the network fails. Line splitting ignores a CRLF split across chunks.
export class TestEventSource extends EventTarget {
  static readonly CONNECTING = 0
  static readonly OPEN = 1
  static readonly CLOSED = 2
  readonly CONNECTING = 0
  readonly OPEN = 1
  readonly CLOSED = 2
  readonly url: string
  readonly withCredentials: boolean
  readyState = 0
  onopen: ((event: Event) => void) | null = null
  onmessage: ((event: MessageEvent) => void) | null = null
  onerror: ((event: Event) => void) | null = null
  #lastEventId = ""
  #reconnectionTime = 3000
  #abort = new AbortController()

  constructor(url: string | URL, init?: EventSourceInit) {
    super()
    this.url = new URL(url, location.href).href
    this.withCredentials = init?.withCredentials ?? false
    this.addEventListener("open", (event) => this.onopen?.(event))
    this.addEventListener("message", (event) =>
      this.onmessage?.(event as MessageEvent),
    )
    this.addEventListener("error", (event) => this.onerror?.(event))
    void this.#connect()
  }

  close() {
    this.readyState = this.CLOSED
    this.#abort.abort()
  }

  async #connect() {
    const headers = new Headers({ accept: "text/event-stream" })
    if (this.#lastEventId) headers.set("last-event-id", this.#lastEventId)
    let response: Response
    try {
      response = await fetch(this.url, {
        headers,
        cache: "no-store",
        credentials: this.withCredentials ? "include" : "same-origin",
        signal: this.#abort.signal,
      })
    } catch {
      return this.#reconnect()
    }
    if (this.readyState === this.CLOSED) return
    const type = response.headers.get("content-type")?.split(";")[0].trim()
    if (response.status !== 200 || type !== "text/event-stream") {
      this.readyState = this.CLOSED
      this.dispatchEvent(new Event("error"))
      return
    }
    this.readyState = this.OPEN
    this.dispatchEvent(new Event("open"))
    try {
      await this.#read(response.body!)
    } catch {
      // Aborted by close() or a broken stream: both end the same way.
    }
    this.#reconnect()
  }

  async #read(body: ReadableStream<Uint8Array>) {
    const reader = body.getReader()
    const decoder = new TextDecoder()
    let buffer = ""
    let data: string[] = []
    let event = ""
    for (;;) {
      const { done, value } = await reader.read()
      if (done) return
      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split(/\r\n|\r|\n/)
      buffer = lines.pop() ?? ""
      for (const line of lines) {
        if (line === "") {
          if (data.length > 0 && this.readyState !== this.CLOSED) {
            this.dispatchEvent(
              new MessageEvent(event || "message", {
                data: data.join("\n"),
                lastEventId: this.#lastEventId,
                origin: new URL(this.url).origin,
              }),
            )
          }
          data = []
          event = ""
          continue
        }
        if (line.startsWith(":")) continue
        const colon = line.indexOf(":")
        const field = colon === -1 ? line : line.slice(0, colon)
        const raw = colon === -1 ? "" : line.slice(colon + 1)
        const text = raw.startsWith(" ") ? raw.slice(1) : raw
        if (field === "data") data.push(text)
        else if (field === "event") event = text
        else if (field === "id" && !text.includes("\0"))
          this.#lastEventId = text
        else if (field === "retry" && /^\d+$/.test(text)) {
          this.#reconnectionTime = Number(text)
        }
      }
    }
  }

  #reconnect() {
    if (this.readyState === this.CLOSED) return
    this.readyState = this.CONNECTING
    this.dispatchEvent(new Event("error"))
    setTimeout(() => {
      if (this.readyState === this.CONNECTING) void this.#connect()
    }, this.#reconnectionTime)
  }
}

if (typeof globalThis.EventSource === "undefined") {
  Object.assign(globalThis, { EventSource: TestEventSource })
}
