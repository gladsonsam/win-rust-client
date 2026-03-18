/** @type {import('tailwindcss').Config} */
export default {
  content: ["./index.html", "./src/**/*.{js,ts,jsx,tsx}"],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        bg: "var(--color-bg)",
        surface: "var(--color-surface)",
        border: "var(--color-border)",
        accent: "var(--color-accent)",
        danger: "var(--color-danger)",
        ok: "var(--color-ok)",
        primary: "var(--color-primary)",
        muted: "var(--color-muted)",
      },
      fontFamily: {
        sans: ['Inter', 'system-ui', 'sans-serif'],
        mono: ["Cascadia Code", "Consolas", "monospace"],
      },
    },
  },
  plugins: [],
};
