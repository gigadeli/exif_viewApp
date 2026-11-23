// 小さな線画アイコン (絵文字はフォントによって崩れるため SVG にする)

const base = {
  width: 16,
  height: 16,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.4,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  "aria-hidden": true,
};

export const ImageIcon = () => (
  <svg {...base}>
    <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
    <circle cx="5.5" cy="6" r="1.2" />
    <path d="M2 12l4-4 3 3 2-2 3.5 3.5" />
  </svg>
);

export const VideoIcon = () => (
  <svg {...base}>
    <rect x="1.5" y="3.5" width="9.5" height="9" rx="1.5" />
    <path d="M11 7l3.5-2v6L11 9z" />
  </svg>
);

export const ChevronDown = () => (
  <svg {...base} width={12} height={12}>
    <path d="M4 6l4 4 4-4" />
  </svg>
);
