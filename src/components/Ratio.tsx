import { GPT_RATIOS, NANO_RATIOS, parseRatio, sizeText } from "../lib/options";
import type { GridPlan } from "../lib/types";

/** 比例小图示：按真实宽高比画的矩形（长边固定），用在下拉项里。auto 画虚线方框。 */
export function RatioIcon({ ratio }: { ratio: string }) {
  const r = parseRatio(ratio);
  const box = 14;
  const [w, h] = r ? (r[0] >= r[1] ? [box, (box * r[1]) / r[0]] : [(box * r[0]) / r[1], box]) : [box, box];
  return (
    <svg viewBox="0 0 16 16" className="size-4 shrink-0 text-text-secondary" aria-hidden>
      <rect
        x={(16 - w) / 2}
        y={(16 - h) / 2}
        width={Math.max(w, 1.2)}
        height={Math.max(h, 1.2)}
        rx={1}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.2}
        strokeDasharray={r ? undefined : "2 1.5"}
      />
    </svg>
  );
}

function ratioLabel(value: string, size?: string) {
  return (
    <span className="flex items-center gap-2">
      <RatioIcon ratio={value} />
      <span>{value === "auto" ? "自动" : value}</span>
      {size && value !== "auto" && <span className="text-text-tertiary">{size}</span>}
    </span>
  );
}

export const gptRatioOptions = GPT_RATIOS.map((r) => ({ value: r.value, label: ratioLabel(r.value, r.size) }));
export const nanoRatioOptions = NANO_RATIOS.map((r) => ({ value: r, label: ratioLabel(r) }));

interface GridPreviewProps {
  plan: GridPlan | null;
  /** 无法规划时的原因（例如 GIF / SVG 不能多合一） */
  error?: string | null;
}

/**
 * 当前比例的预览框：框的形状就是整张图（所选比例）；多合一时画出宫格等分线与每格序号，
 * 下方写整张图尺寸，以及「拆切后每张约 x:y（约 W×H）」与放大后的尺寸。比例为自动时画虚线方框。
 */
export function GridPreview({ plan, error }: GridPreviewProps) {
  if (!plan) return error ? <span className="text-sm text-text-tertiary">{error}</span> : null;
  const multi = plan.count > 1;
  const req = parseRatio(plan.requestRatio);
  const auto = !req;
  // viewBox 用整张比例；preserveAspectRatio 默认 meet，框在 SVG 里按真实比例显示
  const W = 100;
  const H = req ? (100 * req[1]) / req[0] : 100;
  const cw = W / plan.cols;
  const ch = H / plan.rows;
  const stroke = Math.max(W, H) / 120;
  const cells = Array.from({ length: plan.cols * plan.rows }, (_, i) => ({ c: i % plan.cols, r: Math.floor(i / plan.cols), i }));
  const tier = plan.requestTier ? ` · ${plan.requestTier}` : "";
  const dash = `${stroke * 3} ${stroke * 2}`;

  return (
    <div className="flex flex-col gap-1.5">
      <div className="grid place-items-center rounded border bg-fill-alter p-2">
        <svg
          viewBox={`${-stroke} ${-stroke} ${W + 2 * stroke} ${H + 2 * stroke}`}
          className="h-[110px] w-full"
          role="img"
          aria-label={multi ? `预览：整张 ${plan.requestRatio}，${plan.cols} 列 ${plan.rows} 行` : `预览：${plan.requestRatio}`}
        >
          <rect
            x={0}
            y={0}
            width={W}
            height={H}
            className="fill-primary-bg stroke-primary"
            strokeWidth={stroke}
            strokeDasharray={auto ? dash : undefined}
          />
          {multi &&
            cells.map(({ c, r, i }) => (
              <text
                key={i}
                x={c * cw + cw / 2}
                y={r * ch + ch / 2}
                textAnchor="middle"
                dominantBaseline="central"
                fontSize={Math.min(cw, ch) * 0.32}
                className="fill-primary-text"
              >
                {i + 1}
              </text>
            ))}
          {multi &&
            Array.from({ length: plan.cols - 1 }, (_, k) => (
              <line key={`v${k}`} x1={(k + 1) * cw} y1={0} x2={(k + 1) * cw} y2={H} className="stroke-primary" strokeWidth={stroke} strokeDasharray={dash} />
            ))}
          {multi &&
            Array.from({ length: plan.rows - 1 }, (_, k) => (
              <line key={`h${k}`} x1={0} y1={(k + 1) * ch} x2={W} y2={(k + 1) * ch} className="stroke-primary" strokeWidth={stroke} strokeDasharray={dash} />
            ))}
        </svg>
      </div>
      <div className="flex flex-col text-sm text-text-tertiary">
        {auto ? (
          <span>
            比例为「自动」：由模型决定整张图的比例{multi ? `，按 ${plan.cols} 列 × ${plan.rows} 行等分拆切，每张比例取决于实际出图` : "，尺寸出图后才知道"}。
          </span>
        ) : (
          <>
            <span>
              整张 {plan.requestRatio}
              {tier} · 约 {sizeText(plan.requestSize)}
              {multi && ` · ${plan.cols} 列 × ${plan.rows} 行`}
            </span>
            {multi && (
              <span className="text-text-secondary">
                拆切后每张约 {plan.cellRatio}（约 {sizeText(plan.cellSize)}）
                {plan.upscale && plan.outputSize[0] !== plan.cellSize[0] && `，放大到约 ${sizeText(plan.outputSize)}`}
              </span>
            )}
          </>
        )}
      </div>
    </div>
  );
}
