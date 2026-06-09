# Scholium

Solidjs and tauri application for interactive educational books. Based on a description of something that the user desires to learn, will generate a lesson plan and (markdown|html|latex|abstraction) lesson content, including pictures and diagrams if appropriate. 

The user can then interact with that content. Highlight a block of text and say that they don’t understand it, or want more details, or want a diagram or picture. Open a chat to directly ask questions. The Application will then modify the existing lesson with additional clarity, or foot notes or appendices based on the users input.

As the user works through the material, the app should keep track of their position. It should also only generate chapters one at a time based on their progress and interactions.

In the beginning, when the user prompts what they would like to learn, it should check the prompt for specific details and ask them if not found. It should ask the users familiarity with the material and what they currently know about it. It should identify learning that the material depends on and verify the users familiarity with those topics. There should be some determination of the users reading level.

Ideally, the ability to have the book read aloud would be available, better still if there is tracking in the text so the user can read along while having it read. The reading speed should be adjustable.

It is probably wise to figure out some nature of content filtering. Ensure the user isn’t trying to learn something that is dangerous or not ethical.

# Running

Configure the LLM provider and keys in-app via the **Settings** menu (⌘,).
Settings persist to a config file in the app config dir, and the Claude API
key is stored in the OS keyring. The CLI flags below remain available as a
dev override.

`npm run tauri dev`

# Development

## Commands

| Command | Description |
|---|---|
| `npm run dev` | Start Vite dev server (frontend only) |
| `npm run tauri dev` | Run full Tauri app (hot-reload) |
| `npm test` | Run tests once |
| `npm run test:watch` | Run tests in watch mode |
| `npm run build` | Vite production build |
| `npm run tauri build` | Build Tauri app for distribution |

## Checks and formatting

Run all checks before pushing (or let the pre-push hook do it automatically):

```sh
npm run check
```

This runs four checks in sequence:

| Command | What it does |
|---|---|
| `npm run typecheck` | TypeScript type-check (`tsc --noEmit`) |
| `npm run lint:ts` | ESLint with TypeScript + SolidJS rules |
| `npm run lint:fmt` | Prettier format check |
| `npm run lint:rs` | Cargo Clippy with `-D warnings` |

To auto-format everything in one shot:

```sh
npm run fmt
```

This runs `prettier --write` on all TypeScript files and `cargo fmt` on the
Rust codebase.

### Pre-push hook

[lefthook](https://github.com/evilmartians/lefthook) runs all four checks in
parallel on every `git push`. The hook is installed automatically when you run
`npm install` (via the `prepare` script). To reinstall manually:

```sh
npx lefthook install
```

To skip the hook in an emergency:

```sh
LEFTHOOK=0 git push
```
