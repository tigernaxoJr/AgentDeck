<template>
  <div class="flex flex-col gap-6">
    <!-- 1. Choice Mode (True/False or Multiple Choice) -->
    <div v-if="question.inputMode === 'choice'" class="grid grid-cols-2 md:grid-cols-4 gap-4">
      <template v-if="question.qtype === 'true_false'">
        <button
          type="button"
          :class="['h-28 rounded-3xl font-black text-5xl transition-all shadow-md flex items-center justify-center cursor-pointer border-4', selectedChoice === 'O' ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-800' : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800 text-stone-800 dark:text-stone-100']"
          :disabled="disabled"
          @click="selectChoice('O')"
        >
          ⭕ O
        </button>
        <button
          type="button"
          :class="['h-28 rounded-3xl font-black text-5xl transition-all shadow-md flex items-center justify-center cursor-pointer border-4', selectedChoice === 'X' ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-800' : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800 text-stone-800 dark:text-stone-100']"
          :disabled="disabled"
          @click="selectChoice('X')"
        >
          ❌ X
        </button>
      </template>

      <template v-else>
        <button
          v-for="(opt, idx) in question.options"
          :key="idx"
          type="button"
          :class="['p-6 rounded-3xl font-bold text-2xl transition-all shadow-md flex flex-col items-center justify-center gap-2 cursor-pointer border-4 text-center min-h-[110px]', selectedChoice === idx ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-900 dark:text-amber-100' : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800 text-stone-800 dark:text-stone-100']"
          :disabled="disabled"
          @click="selectChoice(idx)"
        >
          <span class="w-10 h-10 rounded-full bg-stone-100 dark:bg-stone-700 text-xl font-black flex items-center justify-center">
            {{ idx + 1 }}
          </span>
          <span class="text-xl md:text-2xl">{{ opt }}</span>
        </button>
      </template>
    </div>

    <!-- 2. Handwriting Pad (CharGrid / FreeCanvas / ImageCanvas) -->
    <div
      v-else-if="['char_grid', 'free_canvas', 'image_canvas'].includes(question.inputMode)"
      class="flex flex-col items-center w-full"
    >
      <HandwritingPad
        ref="padRef"
        :mode="question.inputMode === 'char_grid' ? 'char_grid' : (question.inputMode === 'image_canvas' ? 'image_canvas' : 'free_canvas')"
        :char-count="question.charCount || 2"
        :bg-image-url="question.inputMode === 'image_canvas' ? question.imageUrl : ''"
      />
    </div>

    <!-- 3. Text Input Mode -->
    <div v-else-if="question.inputMode === 'text'" class="w-full">
      <input
        v-model="textInput"
        type="text"
        class="w-full p-6 text-3xl rounded-2xl border-4 border-stone-300 dark:border-stone-600 bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100 focus:border-amber-500 outline-none"
        placeholder="請在此輸入你的答案..."
        :disabled="disabled"
        @keyup.enter="submit"
      />
    </div>

    <!-- Actions Bar -->
    <div class="flex items-center justify-between gap-4 mt-2">
      <div class="flex items-center gap-3">
        <slot name="extra-actions" />
      </div>

      <div class="flex items-center gap-4">
        <BigButton
          v-if="canNext"
          variant="primary"
          @click="$emit('next')"
        >
          下一題 (Enter) ➔
        </BigButton>
        <BigButton
          v-else
          variant="primary"
          :disabled="disabled"
          @click="submit"
        >
          送出答案 (Enter)
        </BigButton>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import type { QuestionView } from '../../shared/types';
import BigButton from '../../shared/BigButton.vue';
import HandwritingPad from './HandwritingPad.vue';

const props = defineProps<{
  question: QuestionView;
  disabled?: boolean;
  canNext?: boolean;
}>();

const emit = defineEmits<{
  (e: 'submit', payload: { kind: 'choice' | 'text' | 'handwriting'; choice?: string | number; text?: string; image_png_base64?: string }): void;
  (e: 'next'): void;
}>();

const selectedChoice = ref<string | number | null>(null);
const textInput = ref('');
const padRef = ref<InstanceType<typeof HandwritingPad> | null>(null);

function selectChoice(val: string | number) {
  selectedChoice.value = val;
}

function submit() {
  if (props.disabled) return;

  if (props.question.inputMode === 'choice') {
    if (selectedChoice.value !== null) {
      emit('submit', {
        kind: 'choice',
        choice: selectedChoice.value,
      });
    }
  } else if (['char_grid', 'free_canvas', 'image_canvas'].includes(props.question.inputMode)) {
    const b64 = padRef.value?.exportPng();
    if (b64) {
      emit('submit', {
        kind: 'handwriting',
        image_png_base64: b64,
      });
    } else {
      alert('請先在畫布上寫字喔！');
    }
  } else if (props.question.inputMode === 'text') {
    if (textInput.value.trim()) {
      emit('submit', {
        kind: 'text',
        text: textInput.value.trim(),
      });
    }
  }
}

defineExpose({
  clear() {
    selectedChoice.value = null;
    textInput.value = '';
    padRef.value?.clear();
  },
  undo() {
    padRef.value?.undo();
  },
  selectChoice,
  submit,
});
</script>
