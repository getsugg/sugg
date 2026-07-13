Bun.serve({
  port: 1731,
  async fetch(req) {
    const url = new URL(req.url);
    const delay = parseInt(url.pathname.slice(1)) || 0;
    if (delay > 0) await new Promise((r) => setTimeout(r, delay));
    return new Response(`delayed ${delay}ms\n`);
  },
});
