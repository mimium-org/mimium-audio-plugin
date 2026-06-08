import { useEffect, useMemo, useRef } from "react";

type DawKnobProps = {
  value: number;
  size?: number;
  onChange: (nextValue: number) => void;
  onDragStart?: () => void;
  onDragEnd?: () => void;
};

const clamp01 = (value: number): number => Math.max(0, Math.min(1, value));

function valueFromDelta(start: number, deltaX: number, deltaY: number): number {
  const sensitivity = 0.0035;
  const delta = deltaX - deltaY;
  return clamp01(start + delta * sensitivity);
}

function drawKnob(canvas: HTMLCanvasElement, value: number): void {
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }

  const dpr = window.devicePixelRatio || 1;
  const cssSize = canvas.clientWidth;
  const size = Math.max(1, Math.floor(cssSize * dpr));
  if (canvas.width !== size || canvas.height !== size) {
    canvas.width = size;
    canvas.height = size;
  }

  ctx.save();
  ctx.scale(dpr, dpr);

  const center = cssSize / 2;
  const radius = cssSize * 0.42;
  const startAngle = Math.PI * 0.75;
  const sweep = Math.PI * 1.5;
  const angle = startAngle + sweep * value;

  ctx.clearRect(0, 0, cssSize, cssSize);

  const outerGradient = ctx.createRadialGradient(
    center,
    center,
    cssSize * 0.06,
    center,
    center,
    radius
  );
  outerGradient.addColorStop(0, "#5c7b8c");
  outerGradient.addColorStop(0.4, "#2f4652");
  outerGradient.addColorStop(1, "#1a2a33");

  ctx.beginPath();
  ctx.arc(center, center, radius, 0, Math.PI * 2);
  ctx.fillStyle = outerGradient;
  ctx.fill();

  ctx.beginPath();
  ctx.arc(center, center, radius - cssSize * 0.04, 0, Math.PI * 2);
  ctx.fillStyle = "#132129";
  ctx.fill();

  ctx.lineWidth = cssSize * 0.06;
  ctx.strokeStyle = "#c4dfed33";
  ctx.beginPath();
  ctx.arc(center, center, radius - cssSize * 0.12, startAngle, startAngle + sweep);
  ctx.stroke();

  ctx.lineWidth = cssSize * 0.06;
  ctx.strokeStyle = "#9bdcff";
  ctx.beginPath();
  ctx.arc(center, center, radius - cssSize * 0.12, startAngle, angle);
  ctx.stroke();

  const indicatorRadius = radius - cssSize * 0.18;
  const ix = center + Math.cos(angle) * indicatorRadius;
  const iy = center + Math.sin(angle) * indicatorRadius;

  ctx.beginPath();
  ctx.arc(ix, iy, cssSize * 0.05, 0, Math.PI * 2);
  ctx.fillStyle = "#d7eff7";
  ctx.fill();

  ctx.restore();
}

export function DawKnob({
  value,
  onChange,
  size = 96,
  onDragStart,
  onDragEnd,
}: DawKnobProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const activePointerId = useRef<number | null>(null);
  const isDraggingRef = useRef(false);
  const dragStartValueRef = useRef(0);
  const dragStartXRef = useRef(0);
  const dragStartYRef = useRef(0);
  const valueRef = useRef(value);
  const onChangeRef = useRef(onChange);
  const onDragStartRef = useRef(onDragStart);
  const onDragEndRef = useRef(onDragEnd);

  const normalized = useMemo(() => clamp01(value), [value]);

  useEffect(() => {
    valueRef.current = value;
  }, [value]);

  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  useEffect(() => {
    onDragStartRef.current = onDragStart;
  }, [onDragStart]);

  useEffect(() => {
    onDragEndRef.current = onDragEnd;
  }, [onDragEnd]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }
    drawKnob(canvas, normalized);
  }, [normalized, size]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) {
      return;
    }

    const onPointerMove = (event: PointerEvent) => {
      if (!isDraggingRef.current || activePointerId.current !== event.pointerId) {
        return;
      }
      const next = valueFromDelta(
        dragStartValueRef.current,
        event.clientX - dragStartXRef.current,
        event.clientY - dragStartYRef.current
      );
      onChangeRef.current(next);
    };

    const onPointerUp = (event: PointerEvent) => {
      if (!isDraggingRef.current || activePointerId.current !== event.pointerId) {
        return;
      }
      isDraggingRef.current = false;
      onDragEndRef.current?.();
      if (activePointerId.current != null) {
        canvas.releasePointerCapture?.(activePointerId.current);
      }
      activePointerId.current = null;
      document.body.style.cursor = "";
    };

    const onPointerDown = (event: PointerEvent) => {
      event.preventDefault();
      dragStartValueRef.current = clamp01(valueRef.current);
      dragStartXRef.current = event.clientX;
      dragStartYRef.current = event.clientY;
      activePointerId.current = event.pointerId;
      isDraggingRef.current = true;
      onDragStartRef.current?.();
      canvas.setPointerCapture(event.pointerId);
      document.body.style.cursor = "ns-resize";
    };

    canvas.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);

    return () => {
      canvas.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      isDraggingRef.current = false;
      activePointerId.current = null;
      document.body.style.cursor = "";
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      className="daw-knob"
      width={size}
      height={size}
      style={{ width: size, height: size }}
      role="slider"
      aria-valuemin={0}
      aria-valuemax={1}
      aria-valuenow={normalized}
      aria-label="Knob"
      tabIndex={0}
      onKeyDown={(event) => {
        if (event.key === "ArrowUp" || event.key === "ArrowRight") {
          event.preventDefault();
          onChange(clamp01(normalized + 0.01));
        }
        if (event.key === "ArrowDown" || event.key === "ArrowLeft") {
          event.preventDefault();
          onChange(clamp01(normalized - 0.01));
        }
      }}
    />
  );
}
