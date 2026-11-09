# TODO

## Features

 - [x] Add _Syntax Highlight_ for Batch (Windows Command Shell).
 - [x] Add option to reset geometry of the application (Check if it affects start up time).
 - [x] Add support for _Drag & Drop_ file into session.  
 - [x] Opening with no file should not open `Untitled` but have clear workspace (Waiting for _Drag & Drop_).
 - [x] Add _Tooltips_ to the icons in _Search Text_ (Both outside the box and within the box). 
 - [x] Make the Right / Down Arrow in _Search Text_ (Replace) larger.
 - [x] Add support for `ini` files with _syntax highlighting_.
 - [x] Add support for Text Spelling with support for text and MarkDown. Should be disabled by default.
 - [ ] Add support for Text Spelling for comments in code.
 - [ ] Add support for `qmd` (Quarto MarkDown) files with _syntax highlighting_.
 - [ ] Add support for Code Spelling like VS Code extensions (Will mark `myVarieble` with support for `snake_case` and `camelCase`).
 - [x] Add "Copy File Name" which copies the file name to clipboard.
 - [x] Add "Copy File Path" which copies the file path to clipboard.
 - [x] Add awareness of 3rd party changes to file and offer to reload.
 - [ ] Add option to do a 3 way merge in case of a 3rd party changes to file (With _Auto_ option in settings).
 - [ ] Add _Auto Indent_ for `C`.
 - [ ] Add _Auto Indent_ for `C++`.
 - [ ] Add _Auto Indent_ for `HTML`.
 - [ ] Add _Auto Indent_ for `JavsScript`.
 - [ ] Add _Auto Indent_ for `Julia`.
 - [ ] Add _Auto Indent_ for `JSON`.
 - [ ] Add _Auto Indent_ for `MATLAB`.
 - [ ] Add _Auto Indent_ for `PowerShell`.
 - [ ] Add _Auto Indent_ for `Python`.
 - [ ] Add _Auto Indent_ for `Rust`.
 - [ ] Add _Auto Indent_ for `TypeScript`.
 - [ ] Add _Auto Indent_ for `XML`.
 - [ ] Add _Auto Indent_ for `YAML`.
 - [ ] Add _Minify_ for `CSS`.
 - [ ] Add _Minify_ for `HTML`.
 - [ ] Add _Minify_ for `JavaScript`.
 - [ ] Add _Minify_ for `JSON`.
 - [ ] Add _Minify_ for `XML`.

## Changes

 - [x] Hint files which are in read only mode (Maybe write the path in orange).
 - [x] Embed metadata into the `exe`: File Version, Product Version, Product Name, File Description.

## Bugs

 - [x] `Untitled` file can not be saved. Yet on close it asks for approval to discard.
 - [x] On documents longer than the screen the scroll bar does not appear when the mouse is scrolling all the way up.

## Performance

 - [ ] Check if the Tree Sitter `C` code can be compiled as `x86-64-v3`.


## Documentation

 - [ ] Add option to scale up the Mermaid diagram.