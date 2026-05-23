import * as monaco from "monaco-editor";

export function registerThemes(): void {
  monaco.editor.defineTheme("mimium-copper", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "keyword.operator.mimium", foreground: "FF9D4D", fontStyle: "bold" },
      { token: "keyword.mimium", foreground: "FFB46A" },
      { token: "type.mimium", foreground: "7FD6C2" },
      { token: "variable.predefined.mimium", foreground: "88D7FF", fontStyle: "italic" },
      { token: "support.function.mimium", foreground: "FFE08A" },
      { token: "number.mimium", foreground: "A5D68A" },
      { token: "string.mimium", foreground: "FFB9A3" },
      { token: "comment.mimium", foreground: "8E9FA8" },
      { token: "operator.mimium", foreground: "EAE1D8" },
    ],
    colors: {
      "editor.background": "#17120F",
      "editor.foreground": "#F7EEE6",
      "editorLineNumber.foreground": "#7E6F64",
      "editorCursor.foreground": "#FFD29D",
      "editor.selectionBackground": "#5B3A2755",
      "editor.lineHighlightBackground": "#2A211D"
    }
  });
}
