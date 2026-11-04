/** @type {import('tailwindcss').Config} */
module.exports = {
  content: ["./index.html", "./renderer.js"],
  theme: {
    extend: {
      colors: {
        bg: "#0e1014", panel: "#161921", panel2: "#1d212b", line: "#272c38",
        ink: "#e7e9ee", muted: "#8a92a3",
        accent: "#7c8cff", accent2: "#b06cff",
        ok: "#4ade80", warn: "#fbbf24", bad: "#f87171",
      },
      fontFamily: {
        sans: ["Inter", "system-ui", "sans-serif"],
        mono: ['"JetBrains Mono"', "ui-monospace", "monospace"],
      },
    },
  },
  plugins: [],
};
