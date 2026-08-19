/** @type {import('tailwindcss').Config} */
export default {
  content: ['./src/**/*.{html,js}'],
  theme: {
    extend: {
      colors: {
        // The school's logo colours, so utilities like `text-brand` match the
        // hand-written component styles exactly.
        brand: {
          DEFAULT: '#F16522',
          50: '#FFF4EE',
          100: '#FFE5D8',
          200: '#FCC8AC',
          600: '#DE551A',
          700: '#B94512',
        },
        ink: {
          DEFAULT: '#333333',
          2: '#5A5F66',
          3: '#868D96',
          4: '#A9B0B8',
        },
      },
      fontFamily: {
        sans: ['Segoe UI', 'system-ui', 'Inter', 'Roboto', 'Helvetica Neue', 'Arial', 'sans-serif'],
        mono: ['Cascadia Mono', 'Consolas', 'SF Mono', 'Menlo', 'monospace'],
      },
    },
  },
  plugins: [],
};
