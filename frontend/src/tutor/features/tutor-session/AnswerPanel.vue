<template>
  <div class="flex flex-col gap-4">
    <!-- 1. Choice Mode (True/False or Multiple Choice) -->
    <div
      v-if="question.inputMode === 'choice'"
      :class="['grid gap-4', isTrueFalse ? 'grid-cols-2' : 'grid-cols-1 sm:grid-cols-2']"
    >
      <button
        v-for="c in choices"
        :key="String(c.value)"
        type="button"
        :class="[
          'relative rounded-3xl border-4 shadow-sm transition-all cursor-pointer disabled:cursor-default active:scale-[0.98]',
          isTrueFalse ? 'h-28 flex items-center justify-center gap-4' : 'p-4 min-h-[88px] flex items-center gap-4 text-left',
          choiceClass(c.value),
        ]"
        :disabled="disabled || locked"
        @click="selectChoice(c.value)"
      >
        <template v-if="isTrueFalse">
          <span class="text-5xl">{{ c.value ? '⭕' : '❌' }}</span>
          <span class="text-4xl font-black">{{ c.label }}</span>
        </template>
        <template v-else>
          <span
            :class="[
              'shrink-0 w-11 h-11 rounded-full text-xl font-black flex items-center justify-center',
              selectedChoice === c.value ? 'bg-current/15' : 'bg-stone-100 dark:bg-stone-700',
            ]"
          >
            {{ c.value }}
          </span>
          <span class="text-2xl font-bold leading-snug">{{ c.label }}</span>
        </template>
        <span
          v-if="markFor(c.value)"
          class="absolute -top-3 -right-3 w-10 h-10 rounded-full flex items-center justify-center text-xl shadow-md bg-white dark:bg-stone-900"
        >
          {{ markFor(c.value) }}
        </span>
      </button>
    </div>

    <!-- 2. Handwriting Pad (CharGrid / FreeCanvas / ImageCanvas) -->
    <div
      v-else-if="isHandwriting"
      class="flex flex-col items-center w-full overflow-x-auto"
    >
      <HandwritingPad
        ref="padRef"
        :mode="question.inputMode as 'char_grid' | 'free_canvas' | 'image_canvas'"
        :char-count="question.charCount || 2"
        :bg-image-url="question.inputMode === 'image_canvas' ? question.imageUrl : ''"
      />
    </div>

    <!-- 3. Text Input Mode -->
    <input
      v-else-if="question.inputMode === 'text'"
      v-model="textInput"
      type="text"
      class="w-full p-5 text-3xl rounded-2xl border-4 border-stone-300 dark:border-stone-600 bg-white dark:bg-stone-800 text-stone-900 dark:text-stone-100 focus:border-amber-500 outline-none disabled:opacity-60"
      placeholder="請在此輸入你的答案..."
      :disabled="disabled || locked"
      @keyup.enter.stop="submit"
    />

    <!-- Actions Bar -->
    <div class="flex items-center justify-between gap-4">
      <p class="text-lg text-stone-500 dark:text-stone-400">
        <template v-if="locked">按 Enter 繼續</template>
        <template v-else-if="question.inputMode === 'choice'">
          {{ isTrueFalse ? '按 O（對）/ X（錯）選擇' : '按 1–' + choices.length + ' 選擇' }}，Enter 送出
        </template>
        <template v-else-if="isHandwriting">寫好後按 Enter 送出</template>
      </p>

      <BigButton
        v-if="locked"
        variant="primary"
        @click="$emit('next')"
      >
        {{ isLast ? '完成複習 🎉' : '下一題 ➔' }}
      </BigButton>
      <BigButton
        v-else
        variant="primary"
        :disabled="disabled || !hasAnswer"
        @click="submit"
      >
        {{ disabled ? '老師批改中…' : '送出答案' }}
      </BigButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue';
import type { ChoiceValue, QuestionView, TutorReply } from '../../shared/types';
import BigButton from '../../shared/BigButton.vue';
import HandwritingPad from './HandwritingPad.vue';
import type { AnswerPayload } from './useTutorSession';

const props = defineProps<{
  question: QuestionView;
  disabled?: boolean;
  /** 本題已揭曉，鎖住作答 */
  locked?: boolean;
  isLast?: boolean;
  verdict?: TutorReply['verdict'];
  /** 揭曉後的正解，用來標出正確選項 */
  correctChoice?: ChoiceValue;
}>();

const emit = defineEmits<{
  (e: 'submit', payload: AnswerPayload): void;
  (e: 'next'): void;
}>();

const selectedChoice = ref<ChoiceValue | null>(null);
/** 最近一次送出的選項；選項改變後就不再顯示對錯標記 */
const submittedChoice = ref<ChoiceValue | null>(null);
const textInput = ref('');
const padRef = ref<InstanceType<typeof HandwritingPad> | null>(null);

const isTrueFalse = computed(() => props.question.qtype === 'true_false');
const isHandwriting = computed(() => ['char_grid', 'free_canvas', 'image_canvas'].includes(props.question.inputMode));

const choices = computed<Array<{ value: ChoiceValue; label: string }>>(() =>
  isTrueFalse.value
    ? [{ value: true, label: '對' }, { value: false, label: '錯' }]
    : props.question.options.map((label, idx) => ({ value: idx + 1, label }))
);

const hasAnswer = computed(() => {
  switch (props.question.inputMode) {
    case 'choice':
      return selectedChoice.value !== null;
    case 'text':
      return !!textInput.value.trim();
    default:
      return padRef.value ? !padRef.value.isEmpty() : false;
  }
});

function showsVerdict(val: ChoiceValue) {
  return val === selectedChoice.value && val === submittedChoice.value && !!props.verdict;
}

function isRevealedCorrect(val: ChoiceValue) {
  return !!props.locked && props.correctChoice === val;
}

function markFor(val: ChoiceValue) {
  if (isRevealedCorrect(val)) return '✅';
  if (!showsVerdict(val)) return '';
  return props.verdict === 'correct' ? '✅' : props.verdict === 'incorrect' ? '❌' : '';
}

function choiceClass(val: ChoiceValue) {
  if (isRevealedCorrect(val) || (showsVerdict(val) && props.verdict === 'correct')) {
    return 'border-emerald-500 bg-emerald-50 text-emerald-900 dark:bg-emerald-950/60 dark:text-emerald-100';
  }
  if (showsVerdict(val) && props.verdict === 'incorrect') {
    return 'border-rose-400 bg-rose-50 text-rose-900 dark:bg-rose-950/50 dark:text-rose-100';
  }
  if (selectedChoice.value === val) {
    return 'border-amber-500 bg-amber-50 text-amber-900 dark:bg-amber-950 dark:text-amber-100 shadow-md';
  }
  return 'border-stone-200 bg-white text-stone-800 hover:border-amber-300 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-100 dark:hover:border-amber-700 disabled:hover:border-stone-200';
}

function selectChoice(val: ChoiceValue) {
  if (props.disabled || props.locked || props.question.inputMode !== 'choice') return;
  if (!choices.value.some((c) => c.value === val)) return;
  selectedChoice.value = val;
}

function submit() {
  if (props.disabled || props.locked || !hasAnswer.value) return;

  if (props.question.inputMode === 'choice') {
    const c = choices.value.find((c) => c.value === selectedChoice.value)!;
    submittedChoice.value = c.value;
    emit('submit', {
      kind: 'choice',
      choice: c.value,
      label: isTrueFalse.value ? `我選「${c.label}」` : `我選 ${c.value}. ${c.label}`,
    });
  } else if (isHandwriting.value) {
    const b64 = padRef.value?.exportPng();
    if (b64) {
      emit('submit', { kind: 'handwriting', image_png_base64: b64, label: '✍️ 我寫好了，請老師看看' });
    }
  } else {
    const text = textInput.value.trim();
    emit('submit', { kind: 'text', text, label: text });
  }
}

defineExpose({
  clear() {
    if (props.locked) return;
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
