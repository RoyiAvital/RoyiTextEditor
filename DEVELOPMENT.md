[![Visitors](https://api.visitorbadge.io/api/combined?path=https%3A%2F%2Fgithub.com%2FRoyiAvital%2FStackExchangeCodes&labelColor=%23f47373&countColor=%23555555&style=plastic)](https://github.com/RoyiAvital/RoyiFileManager) <!-- https://www.visitorbadge.io -->

# Royi Text Editor - Development

## Guidelines

 - Speed
   Most important feature. Time to load empty file, time to load short file, time to show command palette, keystroke rendering delay.  
   Aim for less than 100 [Mili Sec] load time for short file.
 - Single File  
   Editing a single file.
   Includes line numbering.
 - No Menus / Keyboard Focused
   All window is dedicated to text.  
   Command through the _Command Palette_ / _Command Center_.
 - Advanced Search Capabilities  
   Parity with VS Code for Single File search.


## Main Features

 - Syntax Highlighting  
   Efficient Tree Sitter based Syntax Highlighting engine.
 - _Command Center_ / _Command Palette_ Driven  
   Access all commands by <kbd>Ctrl</kbf>+<kbd>Shift</kbf>+<kbd>P</kbf>.  
   Access last 3 commands and favorites commands by <kbd>Ctrl</kbf>+<kbd>P</kbf>.  
 - VS Code Compatibility  
   Replicates VS Code text features: General, Basic Editing, Navigation, Search and Replace, Multi Cursor and Selection.  
   See [VS Code Keyboard Shortcuts Windows](https://code.visualstudio.com/shortcuts/keyboard-shortcuts-windows.pdf).
   No need to implement features which require LSP or related to projects / multi file editing.
 - Read Only Mode
   Supports _read only mode_ through `cli` and command.
 - CLI Parameters  
   Use CLI parameters to configure the session: Read Only.
 - Safety
   Awareness to external editing. Journal edit.


## Design Choices

 - Fonts  
   Using limited number of Monospaced fonts built in.  
 - Customized UI / Text Engine Framework  
   Customized CPU renderer, with persistent piece tree text engine.  
   Frameless window.
 - No IDE
   NO LSP or dynamic code analysis. 
 - Optimized for Use form a File Manager
   No need to remember history or sessions.  
   Main use case is launching it on read only or edit mode by a file manager.
 - Configuration File
   Based on a single `yaml` file next to the executable: `RoyiTextEditor.yaml`.  
   The application only reads it.
 - Left to Right Editing  
   Being focused on code, the editor is LTR only.
 - Modern Architecture  
   Design aims for modern CPU's and Operating System.   
   The CPU requirement is by `x86-64-v3`.
   The OS requirement is Windows 11 22H2 and above (Not strictly forced).
   The Toolchain in use: `x86_64-pc-windows-gnu`.

   

See [Plan/Architecture.md](Plan/Architecture.md) for the full design.

## Alternatives

 - [Dan](https://github.com/dfallman/dan).
 - [Editpad](https://github.com/imluyg/editpad).
 - [FastPad](https://github.com/coccor/Fastpad).
 - [Lite Anvil](https://github.com/danpozmanter/lite-anvil) (See the `nano` variant).
 - [Noter](https://github.com/blisspixel/noter).
 - [Token](https://github.com/HelgeSverre/token).
 - [Fulgur](https://github.com/fulgur-app/Fulgur).