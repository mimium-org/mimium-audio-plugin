import * as monaco from "monaco-editor";

export const LANGUAGE_ID = "mimium";

export function registerMimiumLanguage(): void {
  monaco.languages.register({
    id: LANGUAGE_ID,
    extensions: [".mmm"],
  });

  monaco.languages.setLanguageConfiguration(LANGUAGE_ID, {
    comments: {
      lineComment: "//",
      blockComment: ["/*", "*/"],
    },
    brackets: [["{", "}"], ["[", "]"], ["(", ")"]],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
      { open: "/*", close: "*/" },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
      { open: "'", close: "'" },
    ],
    folding: {
      markers: {
        start: /\{/, 
        end: /\}/,
      },
    },
    indentationRules: {
      increaseIndentPattern: /\{[^}]*$/,
      decreaseIndentPattern: /^\s*\}/,
    },
  });

  monaco.languages.setMonarchTokensProvider(LANGUAGE_ID, {
    defaultToken: "",
    tokenPostfix: ".mimium",
    keywords: ["fn", "let", "letrec", "if", "else", "type", "const", "mod", "use", "pub", "as", "macro", "include"],
    typeKeywords: ["bool", "float", "int", "string", "void"],
    builtinVariables: ["self", "now", "samplerate", "main"],
    builtinFunctions: [
      "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "sinh", "cosh", "tanh", "log", "log2", "log10",
      "exp", "exp2", "pow", "sqrt", "abs", "ceil", "floor", "round", "fmod", "remainder", "min", "max", "sign",
      "rand", "random", "delay", "mem", "print", "println", "probe", "assert_eq", "dsp", "map", "fold", "lift_f"
    ],
    operators: [
      "=", ">", "<", "!", "~", "?", ":", "==", "<=", ">=", "!=", "&&", "||", "++", "--", "+", "-", "*", "/",
      "&", "|", "^", "%", "<<", ">>", "+=", "-=", "*=", "/=", "&=", "|=", "^=", "%=", "<<=", ">>=", "|>", "@"
    ],
    symbols: /[=><!~?:&|+\-*/^%@]+/,
    escapes: /\\(?:[abfnrtv\\"']|x[0-9A-Fa-f]{1,4}|u[0-9A-Fa-f]{4}|U[0-9A-Fa-f]{8})/,
    tokenizer: {
      root: [
        [/#stage\b/, "keyword.other"],
        [/`/, "keyword.operator"],
        [/\$/, "keyword.operator"],
        [/[a-zA-Z_]\w*/, {
          cases: {
            "@keywords": "keyword",
            "@typeKeywords": "type",
            "@builtinVariables": "variable.predefined",
            "@builtinFunctions": "support.function",
            "@default": "identifier"
          }
        }],
        { include: "@whitespace" },
        [/\|>/, "keyword.operator"],
        [/[{}()[\]]/, "@brackets"],
        [/@symbols/, {
          cases: {
            "@operators": "operator",
            "@default": ""
          }
        }],
        [/@/, "constant.language"],
        [/\d*\.\d+([eE][-+]?\d+)?/, "number.float"],
        [/0[xX][0-9a-fA-F]+/, "number.hex"],
        [/0[oO][0-7]+/, "number.octal"],
        [/0[bB][01]+/, "number.binary"],
        [/\d+/, "number"],
        [/[;,.]/, "delimiter"],
        [/"([^"\\]|\\.)*$/, "string.invalid"],
        [/"/, { token: "string.quote", bracket: "@open", next: "@string" }],
        [/'[^\\']'/, "string"],
        [/(\')(@escapes)(\')/, ["string", "string.escape", "string"]],
        [/'/, "string.invalid"]
      ],
      comment: [
        [/[^/*]+/, "comment"],
        [/\/*/, "comment", "@push"],
        [/\*\//, "comment", "@pop"],
        [/[/*]/, "comment"]
      ],
      string: [
        [/[^\\"]+/, "string"],
        [/@escapes/, "string.escape"],
        [/\\./, "string.escape.invalid"],
        [/"/, { token: "string.quote", bracket: "@close", next: "@pop" }]
      ],
      whitespace: [
        [/[ \t\r\n]+/, "white"],
        [/\/\*/, "comment", "@comment"],
        [/\/\/.*$/, "comment"]
      ]
    }
  });
}
