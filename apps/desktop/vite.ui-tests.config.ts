import { defineConfig, mergeConfig } from "vite";
import app from "./vite.config.ts";

/**
 * The app with its fake backend (tests/ui/app.html), built and served as one
 * bundle for the UI tests. The dev server compiles hundreds of modules for
 * every fresh page, which on a two-core CI machine running three browsers
 * took longer than the tests' checks wait.
 */
export default mergeConfig(
  app,
  defineConfig({
    build: {
      outDir: "dist-ui-tests",
      emptyOutDir: true,
      rolldownOptions: {
        input: "tests/ui/app.html",
        output: {
          // Named after their package, as the dev server's paths are, so tests can tell what loaded.
          chunkFileNames: (chunk) => {
            const name = packageOf(chunk.facadeModuleId);
            return name ? `assets/${name}-[name]-[hash].js` : "assets/[name]-[hash].js";
          },
        },
      },
    },
  }),
);

/** The package a module comes from, such as "emojibase-data" or "@scope-name". */
function packageOf(moduleId: string | null): string | null {
  const inPackage = moduleId?.split("/node_modules/").at(-1);
  if (!inPackage || inPackage === moduleId) return null;
  const [first, second] = inPackage.split("/");
  const name = first?.startsWith("@") ? `${first}-${second}` : first;
  return name?.replace(/[^\w.-]/g, "") || null;
}
