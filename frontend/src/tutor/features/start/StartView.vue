<template>
  <div class="min-h-screen bg-[#faf6f0] dark:bg-[#1c1917] text-stone-900 dark:text-stone-100 flex flex-col justify-center py-12">
    <div class="flex items-center justify-center mb-8 gap-4">
      <span class="text-6xl">🧑‍🏫</span>
      <h1 class="text-5xl font-black text-amber-800 dark:text-amber-500">
        AI 家教伴讀複習
      </h1>
    </div>

    <StudentPicker v-if="!currentStudent" @select="currentStudent = $event" />
    <SessionSetup v-else :student="currentStudent" @change-student="currentStudent = null" />

    <div class="flex items-center justify-center gap-6 mt-12 text-lg font-bold text-stone-500">
      <router-link
        v-if="currentStudent"
        :to="`/tutor/report/${currentStudent.id}`"
        class="hover:text-amber-600 underline"
      >
        📊 家長報告
      </router-link>
      <router-link to="/tutor/admin" class="hover:text-amber-600 underline">
        ⚙️ 題庫與 AI 設定
      </router-link>
      <router-link to="/" class="hover:text-stone-700 underline">
        🖥️ 返回開發控制台
      </router-link>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import type { StudentItem } from '../shared/types';
import StudentPicker from './StudentPicker.vue';
import SessionSetup from './SessionSetup.vue';

const currentStudent = ref<StudentItem | null>(null);
</script>
