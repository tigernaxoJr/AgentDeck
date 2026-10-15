<template>
  <div class="min-h-screen bg-[#faf6f0] dark:bg-[#1c1917] text-stone-900 dark:text-stone-100 p-8 max-w-5xl mx-auto flex flex-col gap-8">
    <div class="flex items-center justify-between pb-6 border-b-2 border-stone-200 dark:border-stone-800">
      <h1 class="text-4xl font-black text-amber-800 dark:text-amber-500">
        ⚙️ 家教系統後台管理
      </h1>
      <router-link to="/tutor" class="px-5 py-2.5 rounded-xl bg-stone-200 dark:bg-stone-800 font-bold text-lg">
        返回首頁
      </router-link>
    </div>

    <!-- Tabs -->
    <div class="flex items-center gap-4 border-b-2 border-stone-200 dark:border-stone-800 pb-2">
      <button
        type="button"
        :class="['px-6 py-3 rounded-2xl font-black text-xl cursor-pointer', tab === 'backends' ? 'bg-amber-600 text-white shadow-md' : 'text-stone-500 hover:text-stone-900 dark:hover:text-stone-100']"
        @click="tab = 'backends'"
      >
        AI 模型與 CLI 引擎
      </button>
      <button
        type="button"
        :class="['px-6 py-3 rounded-2xl font-black text-xl cursor-pointer', tab === 'questions' ? 'bg-amber-600 text-white shadow-md' : 'text-stone-500 hover:text-stone-900 dark:hover:text-stone-100']"
        @click="tab = 'questions'"
      >
        題庫與書本頁碼
      </button>
    </div>

    <div>
      <AiBackends v-if="tab === 'backends'" />
      <QuestionAdmin v-else />
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue';
import AiBackends from './AiBackends.vue';
import QuestionAdmin from './QuestionAdmin.vue';

const tab = ref<'backends' | 'questions'>('backends');
</script>
