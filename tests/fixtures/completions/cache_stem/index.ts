import { cache } from "sugg";

export default createCompletion({
  cache_stem: {
    args: dynamic(async () => {
      const val = await cache.get("stem_test", 60000, async () => ["item"]);
      return val || [];
    }),
  },
});
