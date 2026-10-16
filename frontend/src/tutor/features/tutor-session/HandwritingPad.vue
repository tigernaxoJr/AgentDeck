<template>
  <div class="handwriting-pad relative flex flex-col items-center select-none w-full">
    <!-- Controls Toolbar -->
    <div class="flex items-center gap-4 mb-3 w-full justify-between px-2">
      <div class="flex items-center gap-2">
        <button
          type="button"
          :class="['px-4 py-2 rounded-xl font-bold text-lg', tool === 'pen' ? 'bg-amber-600 text-white' : 'bg-stone-200 dark:bg-stone-700 text-stone-800 dark:text-stone-200']"
          @click="tool = 'pen'"
        >
          ✏️ 鉛筆
        </button>
        <button
          type="button"
          :class="['px-4 py-2 rounded-xl font-bold text-lg', tool === 'eraser' ? 'bg-amber-600 text-white' : 'bg-stone-200 dark:bg-stone-700 text-stone-800 dark:text-stone-200']"
          @click="tool = 'eraser'"
        >
          🧹 橡皮擦
        </button>
      </div>

      <div class="flex items-center gap-2">
        <button
          type="button"
          class="px-4 py-2 rounded-xl bg-stone-200 hover:bg-stone-300 dark:bg-stone-700 dark:hover:bg-stone-600 font-bold text-stone-700 dark:text-stone-200"
          @click="undo"
        >
          ↩️ 復原 (Z)
        </button>
        <button
          type="button"
          class="px-4 py-2 rounded-xl bg-stone-200 hover:bg-stone-300 dark:bg-stone-700 dark:hover:bg-stone-600 font-bold text-stone-700 dark:text-stone-200"
          @click="clear"
        >
          🗑️ 清除 (C)
        </button>
      </div>
    </div>

    <!-- Canvas Container -->
    <div
      ref="containerRef"
      class="relative bg-white rounded-2xl border-4 border-stone-300 dark:border-stone-600 shadow-inner overflow-hidden cursor-crosshair touch-none"
      :style="{ width: `${width}px`, height: `${height}px` }"
    >
      <!-- Background Image if image_canvas -->
      <img
        v-if="bgImageUrl"
        ref="bgImgRef"
        :src="bgImageUrl"
        class="absolute inset-0 w-full h-full object-contain pointer-events-none select-none opacity-90"
        alt="Background"
      />

      <!-- Grid lines (visual only, not exported in char_grid mode) -->
      <svg
        v-if="mode === 'char_grid' && gridCount > 0"
        class="absolute inset-0 w-full h-full pointer-events-none opacity-40"
        xmlns="http://www.w3.org/2000/svg"
      >
        <template v-for="i in gridCount" :key="i">
          <!-- Box boundary -->
          <rect
            :x="(i - 1) * gridWidth + 4"
            y="4"
            :width="gridWidth - 8"
            :height="height - 8"
            fill="none"
            stroke="#e06d43"
            stroke-width="2"
          />
          <!-- Cross and diagonal lines (米字格) -->
          <line
            :x1="(i - 1) * gridWidth + gridWidth / 2"
            y1="4"
            :x2="(i - 1) * gridWidth + gridWidth / 2"
            :y2="height - 4"
            stroke="#e06d43"
            stroke-dasharray="4 4"
            stroke-width="1.5"
          />
          <line
            :x1="(i - 1) * gridWidth + 4"
            :y1="height / 2"
            :x2="i * gridWidth - 4"
            :y2="height / 2"
            stroke="#e06d43"
            stroke-dasharray="4 4"
            stroke-width="1.5"
          />
          <line
            :x1="(i - 1) * gridWidth + 4"
            y1="4"
            :x2="i * gridWidth - 4"
            :y2="height - 4"
            stroke="#e06d43"
            stroke-dasharray="4 4"
            stroke-width="1"
          />
          <line
            :x1="i * gridWidth - 4"
            y1="4"
            :x2="(i - 1) * gridWidth + 4"
            :y2="height - 4"
            stroke="#e06d43"
            stroke-dasharray="4 4"
            stroke-width="1"
          />
        </template>
      </svg>

      <!-- Drawing Canvas -->
      <canvas
        ref="canvasRef"
        :width="width"
        :height="height"
        class="absolute inset-0 touch-none"
        @pointerdown="handlePointerDown"
        @pointermove="handlePointerMove"
        @pointerup="handlePointerUp"
        @pointercancel="handlePointerUp"
        @pointerleave="handlePointerLeave"
      />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';

const props = withDefaults(
  defineProps<{
    mode?: 'char_grid' | 'free_canvas' | 'image_canvas';
    charCount?: number;
    bgImageUrl?: string;
    width?: number;
    height?: number;
  }>(),
  {
    mode: 'char_grid',
    charCount: 2,
    bgImageUrl: '',
    width: 600,
    height: 300,
  }
);

const canvasRef = ref<HTMLCanvasElement | null>(null);
const bgImgRef = ref<HTMLImageElement | null>(null);
const containerRef = ref<HTMLDivElement | null>(null);

const tool = ref<'pen' | 'eraser'>('pen');
const strokes = ref<Array<{ tool: 'pen' | 'eraser'; points: Array<{ x: number; y: number; pressure: number }> }>>([]);
let currentStroke: { tool: 'pen' | 'eraser'; points: Array<{ x: number; y: number; pressure: number }> } | null = null;
let isDrawing = false;

const gridCount = computed(() => props.charCount || 2);
const gridWidth = computed(() => props.width / gridCount.value);

onMounted(() => {
  redraw();
});

function handlePointerDown(e: PointerEvent) {
  // Only accept primary button (usually left click / pen touch)
  // For mouse: button must be 0 (left button) and buttons must have bit 1 set (buttons & 1)
  if (e.pointerType === 'mouse' && e.buttons !== 1) return;
  if (e.button !== 0 && !e.isPrimary) return;

  const canvas = canvasRef.value;
  if (!canvas) return;

  try {
    canvas.setPointerCapture(e.pointerId);
  } catch {}

  isDrawing = true;
  const rect = canvas.getBoundingClientRect();
  const x = e.clientX - rect.left;
  const y = e.clientY - rect.top;
  const pressure = e.pressure > 0 ? e.pressure : 0.5;

  currentStroke = {
    tool: tool.value,
    points: [{ x, y, pressure }],
  };
  strokes.value.push(currentStroke);
  drawPoint(x, y, pressure, tool.value);
}

function handlePointerMove(e: PointerEvent) {
  // Guard against drawing if mouse/pen is NOT currently pressed
  if (e.buttons === 0 || (e.pointerType === 'mouse' && (e.buttons & 1) === 0)) {
    if (isDrawing) {
      handlePointerUp(e);
    }
    return;
  }
  if (!isDrawing || !currentStroke) return;
  const canvas = canvasRef.value;
  if (!canvas) return;

  const rect = canvas.getBoundingClientRect();
  const x = e.clientX - rect.left;
  const y = e.clientY - rect.top;
  const pressure = e.pressure > 0 ? e.pressure : 0.5;

  const lastPoint = currentStroke.points[currentStroke.points.length - 1];
  currentStroke.points.push({ x, y, pressure });
  drawLine(lastPoint.x, lastPoint.y, x, y, pressure, currentStroke.tool);
}

function handlePointerUp(e: PointerEvent) {
  if (canvasRef.value) {
    try {
      if (canvasRef.value.hasPointerCapture(e.pointerId)) {
        canvasRef.value.releasePointerCapture(e.pointerId);
      }
    } catch {}
  }
  isDrawing = false;
  currentStroke = null;
}

function handlePointerLeave(e: PointerEvent) {
  // If pointer is not captured and leaves canvas, stop drawing
  if (!canvasRef.value?.hasPointerCapture?.(e.pointerId)) {
    isDrawing = false;
    currentStroke = null;
  }
}

function drawPoint(x: number, y: number, pressure: number, t: 'pen' | 'eraser') {
  const ctx = canvasRef.value?.getContext('2d');
  if (!ctx) return;

  ctx.save();
  if (t === 'eraser') {
    ctx.globalCompositeOperation = 'destination-out';
    ctx.beginPath();
    ctx.arc(x, y, 20, 0, Math.PI * 2);
    ctx.fill();
  } else {
    ctx.globalCompositeOperation = 'source-over';
    ctx.fillStyle = '#000000';
    ctx.beginPath();
    const radius = 3 + pressure * 4;
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.fill();
  }
  ctx.restore();
}

function drawLine(x1: number, y1: number, x2: number, y2: number, pressure: number, t: 'pen' | 'eraser') {
  const ctx = canvasRef.value?.getContext('2d');
  if (!ctx) return;

  ctx.save();
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';
  if (t === 'eraser') {
    ctx.globalCompositeOperation = 'destination-out';
    ctx.lineWidth = 36;
  } else {
    ctx.globalCompositeOperation = 'source-over';
    ctx.strokeStyle = '#000000';
    ctx.lineWidth = 4 + pressure * 6;
  }
  ctx.beginPath();
  ctx.moveTo(x1, y1);
  ctx.lineTo(x2, y2);
  ctx.stroke();
  ctx.restore();
}

function redraw() {
  const canvas = canvasRef.value;
  if (!canvas) return;
  const ctx = canvas.getContext('2d');
  if (!ctx) return;

  ctx.clearRect(0, 0, canvas.width, canvas.height);
  for (const stroke of strokes.value) {
    if (stroke.points.length === 1) {
      drawPoint(stroke.points[0].x, stroke.points[0].y, stroke.points[0].pressure, stroke.tool);
    } else {
      for (let i = 1; i < stroke.points.length; i++) {
        drawLine(
          stroke.points[i - 1].x,
          stroke.points[i - 1].y,
          stroke.points[i].x,
          stroke.points[i].y,
          stroke.points[i].pressure,
          stroke.tool
        );
      }
    }
  }
}

function undo() {
  if (strokes.value.length > 0) {
    strokes.value.pop();
    redraw();
  }
}

function clear() {
  strokes.value = [];
  redraw();
}

function isEmpty(): boolean {
  return strokes.value.length === 0;
}

function exportPng(): string | null {
  if (isEmpty()) return null;

  const canvas = canvasRef.value;
  if (!canvas) return null;

  // Create an offscreen export canvas
  const exportCanvas = document.createElement('canvas');
  let exportW = canvas.width;
  let exportH = canvas.height;

  // Scale down if longest side > 1024
  const maxDim = Math.max(exportW, exportH);
  let scale = 1;
  if (maxDim > 1024) {
    scale = 1024 / maxDim;
    exportW = Math.round(exportW * scale);
    exportH = Math.round(exportH * scale);
  }

  exportCanvas.width = exportW;
  exportCanvas.height = exportH;
  const ctx = exportCanvas.getContext('2d')!;

  // Fill white background
  ctx.fillStyle = '#ffffff';
  ctx.fillRect(0, 0, exportW, exportH);

  // If image_canvas, draw background image
  if (props.mode === 'image_canvas' && bgImgRef.value && bgImgRef.value.complete) {
    ctx.drawImage(bgImgRef.value, 0, 0, exportW, exportH);
  }

  // Draw strokes
  ctx.drawImage(canvas, 0, 0, exportW, exportH);

  const dataUrl = exportCanvas.toDataURL('image/png');
  return dataUrl.replace(/^data:image\/png;base64,/, '');
}

defineExpose({
  undo,
  clear,
  isEmpty,
  exportPng,
});
</script>
