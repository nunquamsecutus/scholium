# Edu-Harness

Solidjs and tauri application for interactive educational books. Based on a description of something that the user desires to learn, will generate a lesson plan and (markdown|html|latex|abstraction) lesson content, including pictures and diagrams if appropriate. 

The user can then interact with that content. Highlight a block of text and say that they don’t understand it, or want more details, or want a diagram or picture. Open a chat to directly ask questions. The Application will then modify the existing lesson with additional clarity, or foot notes or appendices based on the users input.

As the user works through the material, the app should keep track of their position. It should also only generate chapters one at a time based on their progress and interactions.

In the beginning, when the user prompts what they would like to learn, it should check the prompt for specific details and ask them if not found. It should ask the users familiarity with the material and what they currently know about it. It should identify learning that the material depends on and verify the users familiarity with those topics. There should be some determination of the users reading level.

Ideally, the ability to have the book read aloud would be available, better still if there is tracking in the text so the user can read along while having it read. The reading speed should be adjustable.

It is probably wise to figure out some nature of content filtering. Ensure the user isn’t trying to learn something that is dangerous or not ethical.

# Running

## Claude
`npm run tauri dev -- -- --api-key sk-ant-...`
or just set the env var
`ANTHROPIC_API_KEY=sk-ant-... npm run tauri dev`

# Ollama
`npm run tauri dev -- -- --ollama-url http://localhost:11434 --ollama-model mistral`
  
