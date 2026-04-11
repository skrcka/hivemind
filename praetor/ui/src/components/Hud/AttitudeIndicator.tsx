import type { Attitude } from "../../lib/tauri";

interface Props {
  attitude: Attitude;
  size?: number;
}

/**
 * Classic artificial horizon. Sky above, ground below, tilted by roll,
 * translated by pitch. Pure SVG — ~80 lines, no library.
 */
export function AttitudeIndicator({ attitude, size = 320 }: Props) {
  const pitchDeg = (attitude.pitch_rad * 180) / Math.PI;
  const rollDeg = (attitude.roll_rad * 180) / Math.PI;

  // How much vertical shift in SVG units per degree of pitch. Picked so
  // that ±30° fits nicely inside the circle.
  const pitchShiftPerDeg = size / 90;
  const pitchShift = pitchDeg * pitchShiftPerDeg;

  const half = size / 2;
  const radius = half - 4;

  return (
    <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`}>
      <defs>
        <clipPath id="horizon-clip">
          <circle cx={half} cy={half} r={radius} />
        </clipPath>
      </defs>

      {/* Rotating + shifting inner group */}
      <g
        clipPath="url(#horizon-clip)"
        transform={`rotate(${-rollDeg} ${half} ${half}) translate(0 ${pitchShift})`}
      >
        {/* Sky */}
        <rect x={-size} y={-size} width={size * 3} height={size * 2} fill="#2d6fc3" />
        {/* Ground */}
        <rect x={-size} y={half} width={size * 3} height={size * 2} fill="#8b5a2b" />
        {/* Horizon line */}
        <line
          x1={-size}
          y1={half}
          x2={size * 2}
          y2={half}
          stroke="white"
          strokeWidth="2"
        />
        {/* Pitch ladder */}
        {[-60, -45, -30, -20, -10, 10, 20, 30, 45, 60].map((p) => {
          const y = half - p * pitchShiftPerDeg;
          const w = Math.abs(p) % 30 === 0 ? 80 : 40;
          return (
            <g key={p}>
              <line
                x1={half - w / 2}
                y1={y}
                x2={half + w / 2}
                y2={y}
                stroke="white"
                strokeWidth="1"
              />
              <text
                x={half - w / 2 - 6}
                y={y + 4}
                fill="white"
                fontSize="10"
                textAnchor="end"
              >
                {Math.abs(p)}
              </text>
            </g>
          );
        })}
      </g>

      {/* Static outer ring */}
      <circle
        cx={half}
        cy={half}
        r={radius}
        fill="none"
        stroke="#1f2937"
        strokeWidth="3"
      />

      {/* Static aircraft symbol */}
      <g stroke="#facc15" strokeWidth="3" fill="#facc15">
        <line x1={half - 60} y1={half} x2={half - 20} y2={half} />
        <line x1={half + 20} y1={half} x2={half + 60} y2={half} />
        <circle cx={half} cy={half} r="3" />
      </g>

      {/* Roll scale */}
      <g stroke="white" strokeWidth="1" fill="white" fontSize="9">
        {[-60, -45, -30, -15, 0, 15, 30, 45, 60].map((r) => {
          const angle = (r - 90) * (Math.PI / 180);
          const x1 = half + (radius - 2) * Math.cos(angle);
          const y1 = half + (radius - 2) * Math.sin(angle);
          const x2 = half + (radius - 10) * Math.cos(angle);
          const y2 = half + (radius - 10) * Math.sin(angle);
          return <line key={r} x1={x1} y1={y1} x2={x2} y2={y2} />;
        })}
      </g>
    </svg>
  );
}
