// Animated loading icon: an open book with a page turning across the spine.
// The flip animation lives in App.css (.book-loader-page / @keyframes pageflip).
export default function BookLoader(props: { class?: string }) {
  return (
    <svg
      class={`book-loader${props.class ? ` ${props.class}` : ""}`}
      viewBox="0 0 48 48"
      xmlns="http://www.w3.org/2000/svg"
      aria-hidden="true"
    >
      <g class="book-loader-frame">
        <path d="M5 13 H23 V37 H5 Z" />
        <path d="M25 13 H43 V37 H25 Z" />
        <line x1="24" y1="11" x2="24" y2="39" />
      </g>
      <path class="book-loader-page" d="M24 14 H40 V36 H24 Z" />
    </svg>
  );
}
