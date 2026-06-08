import * as monaco from "monaco-editor";

export function registerThemes(): void {
  monaco.editor.defineTheme("mimium-copper", {
    base: "vs-dark",
    inherit: true,
    rules: [
      { token: "keyword.operator.mimium", foreground: "8ED8FF", fontStyle: "bold" },
      { token: "keyword.mimium", foreground: "A4E4FF" },
      { token: "type.mimium", foreground: "95E7D8" },
      { token: "variable.predefined.mimium", foreground: "B7E8FF", fontStyle: "italic" },
      { token: "support.function.mimium", foreground: "C8EFFF" },
      { token: "number.mimium", foreground: "BCE6A6" },
      { token: "string.mimium", foreground: "AFC8FF" },
      { token: "comment.mimium", foreground: "7E93A1" },
      { token: "operator.mimium", foreground: "DCEAF2" },
    ],
    colors: {
      "editor.background": "#1B2A31",
      "editor.foreground": "#E8F3FA",
      "editorLineNumber.foreground": "#7F98A8",
      "editorCursor.foreground": "#A6E4FF",
      "editor.selectionBackground": "#5FAFD366",
      "editor.lineHighlightBackground": "#243740"
    }
  });
}
