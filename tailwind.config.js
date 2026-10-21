/** @type {import('tailwindcss').Config} */
export default {
  darkMode: 'class',
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      colors: {
        vault: {
          bg: '#06101c',
          panel: '#091a29',
          card: '#0b1c2b',
          surface: '#071421',
          border: '#1b3a53',
          borderHover: '#24465f',
          accent: '#22d3ee',
          accentHover: '#67e8f9',
          accentDark: '#0891b2',
          text: '#dceaf6',
          textMuted: '#8da5b8',
          textDim: '#587184',
          danger: '#fb7185',
          dangerDark: '#9f2941',
          success: '#5eead4',
          successDark: '#0b302f',
          warning: '#f8d584',
          userBubble: '#0c2435',
        },
      },
      fontFamily: {
        sans: [
          'Microsoft YaHei UI',
          'Segoe UI',
          '-apple-system',
          'BlinkMacSystemFont',
          'sans-serif',
        ],
        mono: [
          'Cascadia Code',
          'Consolas',
          'Courier New',
          'monospace',
        ],
      },
    },
  },
  plugins: [],
};
