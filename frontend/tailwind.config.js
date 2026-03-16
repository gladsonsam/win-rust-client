/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        bg:      '#0f1117',
        surface: '#1a1d27',
        border:  '#2a2d3a',
        accent:  '#5b8def',
        danger:  '#e05c5c',
        ok:      '#4caf78',
        primary: '#d4d8f0',
        muted:   '#6b7090',
      },
      fontFamily: {
        mono: ['Cascadia Code', 'Consolas', 'monospace'],
      },
    },
  },
  plugins: [],
}
