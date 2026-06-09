import tseslint from "typescript-eslint";
import solid from "eslint-plugin-solid";
import prettier from "eslint-config-prettier";

export default tseslint.config(
  { ignores: ["dist/**", "node_modules/**", "src-tauri/**"] },
  tseslint.configs.recommended,
  solid.configs["flat/typescript"],
  prettier,
  {
    // BookView renders HTML produced by our Rust markdown renderer, not raw
    // user input — innerHTML is intentional and the source is controlled.
    files: ["src/components/BookView.tsx"],
    rules: { "solid/no-innerhtml": "off" },
  },
);
