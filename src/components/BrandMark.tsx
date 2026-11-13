/**
 * 侧栏品牌标记：与应用图标（src-tauri/app-icon.svg）同一图形——画框山水 + 星光笔触。
 * 单色 currentColor，颜色由 AppShell 的反色块决定，随主题变化。
 */
export function BrandMark() {
  return (
    <svg viewBox="0 0 64 64" className="size-7" aria-hidden>
      <rect x="6" y="14" width="44" height="40" rx="7" fill="none" stroke="currentColor" strokeWidth="4.5" />
      <circle cx="37" cy="27" r="4.2" fill="currentColor" />
      <path d="M8 49 L21 34 Q23 32 25 34 L33 43 L38 38 Q40 36 42 38 L48 44 L48 52 L8 52 Z" fill="currentColor" />
      <path d="M33 22 Q42 15 50 12" fill="none" stroke="currentColor" strokeWidth="3.5" strokeLinecap="round" />
      <path d="M52 2 C53 7 54.5 8.5 59.5 9.5 C54.5 10.5 53 12 52 17 C51 12 49.5 10.5 44.5 9.5 C49.5 8.5 51 7 52 2 Z" fill="currentColor" />
    </svg>
  );
}
