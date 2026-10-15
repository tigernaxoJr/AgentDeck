<template>
  <div class="flex flex-col items-center gap-8 max-w-4xl w-full mx-auto p-6">
    <h2 class="text-4xl font-black text-amber-800 dark:text-amber-400">
      請選擇學童
    </h2>

    <div class="grid grid-cols-2 md:grid-cols-3 gap-6 w-full">
      <button
        v-for="s in students"
        :key="s.id"
        type="button"
        class="p-8 rounded-3xl bg-white dark:bg-stone-800 border-4 border-stone-200 dark:border-stone-700 hover:border-amber-500 shadow-md flex flex-col items-center gap-4 cursor-pointer hover:scale-102 transition-all"
        @click="$emit('select', s)"
      >
        <div class="w-24 h-24 rounded-full bg-amber-100 dark:bg-amber-900 flex items-center justify-center text-5xl">
          🧑‍🎓
        </div>
        <span class="text-3xl font-black">{{ s.name }}</span>
        <span class="text-lg text-stone-500 font-bold">{{ s.grade }} 年級</span>
      </button>

      <button
        type="button"
        class="p-8 rounded-3xl bg-stone-100 dark:bg-stone-850 border-4 border-dashed border-stone-300 dark:border-stone-600 hover:border-amber-500 shadow-sm flex flex-col items-center justify-center gap-4 cursor-pointer hover:scale-102 transition-all text-stone-600 dark:text-stone-300"
        @click="showAdd = true"
      >
        <div class="text-5xl">➕</div>
        <span class="text-2xl font-bold">新增學童</span>
      </button>
    </div>

    <!-- Modal to add student -->
    <div
      v-if="showAdd"
      class="fixed inset-0 z-50 bg-black/60 flex items-center justify-center p-4"
    >
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-8 max-w-md w-full shadow-2xl flex flex-col gap-6">
        <h3 class="text-3xl font-black">新增學童</h3>
        <input
          v-model="newName"
          type="text"
          class="p-4 text-2xl rounded-2xl border-2 border-stone-300 dark:border-stone-600 bg-stone-50 dark:bg-stone-900 outline-none"
          placeholder="學童姓名"
        />
        <div class="flex items-center gap-4">
          <label class="text-xl font-bold">年級：</label>
          <select
            v-model.number="newGrade"
            class="p-3 text-xl rounded-xl border-2 border-stone-300 dark:border-stone-600 bg-stone-50 dark:bg-stone-900 outline-none"
          >
            <option :value="3">三年級</option>
            <option :value="4">四年級</option>
            <option :value="5">五年級</option>
            <option :value="6">六年級</option>
          </select>
        </div>
        <div class="flex items-center justify-end gap-4">
          <button
            type="button"
            class="px-5 py-3 rounded-xl bg-stone-200 dark:bg-stone-700 font-bold"
            @click="showAdd = false"
          >
            取消
          </button>
          <BigButton variant="primary" @click="addStudent">
            確認新增
          </BigButton>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import type { StudentItem } from '../../shared/types';
import * as api from './api';
import BigButton from '../../shared/BigButton.vue';

const emit = defineEmits<{
  (e: 'select', student: StudentItem): void;
}>();

const students = ref<StudentItem[]>([]);
const showAdd = ref(false);
const newName = ref('');
const newGrade = ref(4);

onMounted(async () => {
  load();
});

async function load() {
  students.value = await api.fetchStudents();
}

async function addStudent() {
  if (!newName.value.trim()) return;
  const s = await api.createStudent(newName.value.trim(), newGrade.value);
  newName.value = '';
  showAdd.value = false;
  await load();
  emit('select', s);
}
</script>
