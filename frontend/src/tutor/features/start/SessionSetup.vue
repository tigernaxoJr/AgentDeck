<template>
  <div class="flex flex-col gap-8 max-w-4xl w-full mx-auto p-6">
    <div class="flex items-center justify-between">
      <h2 class="text-3xl font-black">
        👦 {{ student.name }} ({{ student.grade }} 年級) 的複習設定
      </h2>
      <button
        type="button"
        class="text-amber-600 font-bold text-lg hover:underline"
        @click="$emit('change-student')"
      >
        更換學童
      </button>
    </div>

    <!-- 1. Select Subject -->
    <div class="flex flex-col gap-4">
      <span class="text-2xl font-bold">1. 選擇科目</span>
      <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
        <button
          v-for="sub in subjects"
          :key="sub.id"
          type="button"
          :class="[
            'p-6 rounded-3xl font-black text-3xl border-4 transition-all shadow-md cursor-pointer flex flex-col items-center justify-center gap-2',
            selectedSubjectId === sub.id
              ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-900 dark:text-amber-100'
              : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800'
          ]"
          @click="selectedSubjectId = sub.id"
        >
          <span>{{ sub.name }}</span>
          <span class="text-sm font-normal text-stone-500 dark:text-stone-400">{{ sub.bookTitle }}</span>
        </button>
      </div>
    </div>

    <!-- 2. Select Mode -->
    <div class="flex flex-col gap-4">
      <span class="text-2xl font-bold">2. 選擇模式</span>
      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <button
          type="button"
          :class="[
            'p-6 rounded-3xl font-bold text-2xl border-4 transition-all shadow-md cursor-pointer flex flex-col items-start gap-2',
            mode === 'progress'
              ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-900 dark:text-amber-100'
              : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800'
          ]"
          @click="mode = 'progress'"
        >
          <div class="text-3xl font-black">📖 循序新進度</div>
          <p class="text-base font-normal text-stone-600 dark:text-stone-300">
            依單元題目順序練習，跳過已熟練的題目。
          </p>
        </button>

        <button
          type="button"
          :class="[
            'p-6 rounded-3xl font-bold text-2xl border-4 transition-all shadow-md cursor-pointer flex flex-col items-start gap-2',
            mode === 'review'
              ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-900 dark:text-amber-100'
              : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800'
          ]"
          @click="mode = 'review'"
        >
          <div class="text-3xl font-black">🎯 弱點複習</div>
          <p class="text-base font-normal text-stone-600 dark:text-stone-300">
            間隔複習到期與概念弱點題目。
          </p>
        </button>

        <button
          type="button"
          :class="[
            'p-6 rounded-3xl font-bold text-2xl border-4 transition-all shadow-md cursor-pointer flex flex-col items-start gap-2',
            mode === 'mixed'
              ? 'border-amber-500 bg-amber-100 dark:bg-amber-950 text-amber-900 dark:text-amber-100'
              : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800'
          ]"
          @click="mode = 'mixed'"
        >
          <div class="text-3xl font-black">🔀 綜合混合</div>
          <p class="text-base font-normal text-stone-600 dark:text-stone-300">
            60% 複習弱點 + 40% 新進度。
          </p>
        </button>
      </div>
    </div>

    <!-- 3. Select Unit if Progress Mode -->
    <div v-if="mode === 'progress' && currentSubject" class="flex flex-col gap-4">
      <span class="text-2xl font-bold">3. 選擇單元</span>
      <div class="grid grid-cols-1 md:grid-cols-2 gap-3 max-h-60 overflow-y-auto">
        <button
          v-for="u in currentSubject.units"
          :key="u.id"
          type="button"
          :class="[
            'p-4 rounded-2xl border-2 text-left font-bold text-lg flex items-center justify-between',
            selectedUnitId === u.id
              ? 'border-amber-500 bg-amber-50 dark:bg-amber-950/60'
              : 'border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-800'
          ]"
          @click="selectedUnitId = u.id"
        >
          <span>{{ u.title }}</span>
          <span class="text-sm font-normal text-stone-500">熟練 {{ u.mastered }} / {{ u.total }}</span>
        </button>
      </div>
    </div>

    <!-- Start Button -->
    <div class="flex justify-end mt-4">
      <BigButton variant="primary" :disabled="starting" @click="handleStart">
        🚀 開始複習！
      </BigButton>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import { useRouter } from 'vue-router';
import type { StudentItem, SubjectCatalogItem } from '../../shared/types';
import * as api from './api';
import BigButton from '../../shared/BigButton.vue';

const props = defineProps<{
  student: StudentItem;
}>();

defineEmits<{
  (e: 'change-student'): void;
}>();

const router = useRouter();
const subjects = ref<SubjectCatalogItem[]>([]);
const selectedSubjectId = ref('social');
const mode = ref<'progress' | 'review' | 'mixed'>('progress');
const selectedUnitId = ref('');
const starting = ref(false);

const currentSubject = computed(() => {
  return subjects.value.find((s) => s.id === selectedSubjectId.value);
});

onMounted(async () => {
  subjects.value = await api.fetchSubjects(props.student.id);
  if (subjects.value.length > 0) {
    selectedSubjectId.value = subjects.value[0].id;
    if (subjects.value[0].units.length > 0) {
      selectedUnitId.value = subjects.value[0].units[0].id;
    }
  }
});

async function handleStart() {
  starting.value = true;
  try {
    const s = await api.startSession({
      student_id: props.student.id,
      mode: mode.value,
      subject_id: selectedSubjectId.value,
      unit_id: mode.value === 'progress' ? selectedUnitId.value : undefined,
    });

    if (s.empty) {
      alert(s.message || '目前沒有需要複習的題目 🎉');
      return;
    }

    router.push(`/tutor/session/${s.sessionId}`);
  } catch (e: any) {
    alert(e.message || '啟動複習失敗');
  } finally {
    starting.value = false;
  }
}
</script>
