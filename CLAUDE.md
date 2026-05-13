# Edu-harness

SolidJS + Tauri desktop app for AI-generated interactive educational books.

## Development workflow

- Write tests for new functionality using Vitest + `@solidjs/testing-library`.
- Run `npm test` and confirm all tests pass before committing.
- Commit changes with a clear message once tests are green.

## Commands

| Command | Description |
|---|---|
| `npm test` | Run tests once |
| `npm run test:watch` | Run tests in watch mode |
| `npm run dev` | Start Vite dev server |
| `npm run tauri dev` | Run full Tauri app (dev) |
| `npm run build` | Vite production build |
| `npm run tauri build` | Build Tauri app for distribution |

## Project structure

- `src/` — SolidJS frontend (TypeScript)
- `src-tauri/` — Rust backend (Tauri)
- `src/test-setup.ts` — Vitest global setup (jest-dom matchers)

## Stack

- **Frontend**: SolidJS, TypeScript, Vite
- **Desktop shell**: Tauri v2 (Rust)
- **Tests**: Vitest, @solidjs/testing-library, @testing-library/jest-dom
