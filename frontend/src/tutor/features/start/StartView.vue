<template>
  <div class="h-screen overflow-y-auto bg-[#faf6f0] dark:bg-[#1c1917] text-stone-900 dark:text-stone-100 flex flex-col px-4 pb-28">
    <!-- Top Navigation Bar -->
    <header class="max-w-4xl w-full mx-auto flex items-center justify-between py-4 border-b border-stone-200 dark:border-stone-800">
      <div class="flex items-center gap-3">
        <span class="text-3xl">🧑‍🏫</span>
        <h1 class="text-2xl md:text-3xl font-black text-amber-800 dark:text-amber-500">
          AI 家教伴讀
        </h1>
      </div>

      <nav class="flex items-center gap-3 md:gap-5 text-sm md:text-base font-bold text-stone-600 dark:text-stone-400">
        <router-link
          v-if="currentStudent"
          :to="`/tutor/report/${currentStudent.id}`"
          class="hover:text-amber-600 transition-colors"
        >
          📊 家長報告
        </router-link>
        <router-link to="/tutor/admin" class="hover:text-amber-600 transition-colors">
          ⚙️ 題庫與 AI
        </router-link>
        <router-link to="/" class="hover:text-stone-800 dark:hover:text-stone-200 transition-colors">
          🖥️ 控制台
        </router-link>
      </nav>
    </header>

    <main class="flex-1 flex flex-col justify-start my-4">
      <StudentPicker v-if="!currentStudent" @select="currentStudent = $event" />
      <SessionSetup v-else :student="currentStudent" @change-student="currentStudent = null" />
    </main>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import type { StudentItem } from '../shared/types';
import StudentPicker from './StudentPicker.vue';
import SessionSetup from './SessionSetup.vue';

const currentStudent = ref<StudentItem | null>(null);
</script>
