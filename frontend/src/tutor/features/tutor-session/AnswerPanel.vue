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
        :aria-pressed="selectedChoice === c.value"
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
              selectedChoice === c.value && !locked ? 'bg-amber-500 text-white' : 'bg-stone-100 dark:bg-stone-700',
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
    <HandwritingPad
      v-else-if="isHandwriting"
      ref="padRef"
      :mode="question.inputMode as 'char_grid' | 'free_canvas' | 'image_canvas'"
      :char-count="question.charCount || 2"
      :bg-image-url="question.inputMode === 'image_canvas' ? question.imageUrl : ''"
      :disabled="disabled || locked"
    />

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
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import type { ChoiceValue, QuestionView, TutorReply } from '../../shared/types';
import HandwritingPad from './HandwritingPad.vue';
import type { AnswerPayload } from './useTutorSession';

const props = defineProps<{
  question: QuestionView;
  disabled?: boolean;
  /** 本題已揭曉，鎖住作答 */
  locked?: boolean;
  /** 最近一次批改結果；揭曉時 reveal.choice 用來標出正確選項 */
  reply?: TutorReply | null;
}>();

const emit = defineEmits<{
  (e: 'submit', payload: AnswerPayload): void;
}>();

const selectedChoice = ref<ChoiceValue | null>(null);
/** 最近一次送出的選項，等批改結果回來後歸類 */
const submittedChoice = ref<ChoiceValue | null>(null);
/** 本題已經答錯過的選項，重試時保留 ❌ 提醒 */
const wrongChoices = ref<ChoiceValue[]>([]);
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

/** 鍵盤提示，顯示在作答列 */
const hint = computed(() => {
  switch (props.question.inputMode) {
    case 'choice':
      return `${isTrueFalse.value ? '按 O（對）/ X（錯）選擇' : `按 1–${choices.value.length} 選擇`}，Enter 送出`;
    case 'text':
      return '打好後按 Enter 送出';
    default:
      return '寫好後按 Enter 送出，Z 復原、C 清除';
  }
});

const verdict = computed(() => props.reply?.verdict);

watch(
  () => props.reply,
  (r) => {
    if (r?.verdict !== 'incorrect' || submittedChoice.value === null) return;
    if (!wrongChoices.value.includes(submittedChoice.value)) wrongChoices.value.push(submittedChoice.value);
    // 答錯就取消選取，讓孩子重新選
    if (selectedChoice.value === submittedChoice.value) selectedChoice.value = null;
  }
);

const isCorrectPick = (val: ChoiceValue) =>
  verdict.value === 'correct' && val === submittedChoice.value && val === selectedChoice.value;

const isRevealedCorrect = (val: ChoiceValue) => !!props.locked && props.reply?.reveal?.choice === val;

function markFor(val: ChoiceValue) {
  if (isRevealedCorrect(val) || isCorrectPick(val)) return '✅';
  if (wrongChoices.value.includes(val)) return '❌';
  return '';
}

function choiceClass(val: ChoiceValue) {
  if (isRevealedCorrect(val) || isCorrectPick(val)) {
    return 'border-emerald-500 bg-emerald-50 text-emerald-900 ring-4 ring-emerald-200 dark:bg-emerald-950/60 dark:text-emerald-100 dark:ring-emerald-900';
  }
  if (selectedChoice.value === val && !props.locked) {
    return 'border-amber-500 bg-amber-100 text-amber-950 ring-4 ring-amber-200 shadow-md dark:bg-amber-950 dark:text-amber-100 dark:ring-amber-900';
  }
  if (wrongChoices.value.includes(val)) {
    return 'border-rose-300 bg-rose-50 text-rose-900/70 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-200/70';
  }
  if (props.locked) {
    return 'border-stone-200 bg-white text-stone-400 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-500';
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
  hasAnswer,
  hint,
  clear() {
    if (props.locked) return;
    selectedChoice.value = null;
    textInput.value = '';
    padRef.value?.clear();
  },
  undo() {
    if (props.locked) return;
    padRef.value?.undo();
  },
  selectChoice,
  submit,
});
</script>
