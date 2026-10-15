<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="text-2xl font-black">題庫管理與書本頁碼校對</h2>
      <button
        type="button"
        class="px-5 py-2.5 rounded-xl bg-amber-600 text-white font-bold hover:bg-amber-700"
        @click="triggerEnrichment"
      >
        ✨ 啟動 AI 頁碼與位置補強 (Enrich)
      </button>
    </div>

    <!-- Question edit table / card list -->
    <div class="bg-white dark:bg-stone-850 rounded-3xl p-6 shadow-sm border border-stone-200 dark:border-stone-700">
      <p class="text-lg text-stone-600 dark:text-stone-300 mb-4">
        目前系統共有 54 道社會/自然題庫題目，已支援實體書印刷頁碼及課本切片插圖對照。
      </p>
      <div class="text-sm text-stone-500 font-mono">
        提示：若有需要微調特定題目的課本印刷頁碼與書本位置，可直接透過 PATCH /api/tutor/admin/questions/:id 進行更新。
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import * as api from './api';

async function triggerEnrichment() {
  if (!confirm('確定要針對「社會」科啟動背景掃描頁 AI 辨識補強嗎？')) return;
  try {
    await api.triggerEnrich('social');
    alert('AI 書本頁碼補強工作已於背景啟動！');
  } catch (e: any) {
    alert(e.message || '啟動失敗');
  }
}
</script>
