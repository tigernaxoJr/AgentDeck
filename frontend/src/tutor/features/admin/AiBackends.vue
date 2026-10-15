<template>
  <div class="flex flex-col gap-6">
    <div class="flex items-center justify-between">
      <h2 class="text-2xl font-black">AI 後端模型管理與狀態探測</h2>
      <button
        type="button"
        class="px-5 py-2.5 rounded-xl bg-amber-600 text-white font-bold hover:bg-amber-700"
        @click="saveBackends"
      >
        儲存設定
      </button>
    </div>

    <!-- Available Backends List -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
      <div
        v-for="b in backends.available"
        :key="b.id"
        class="p-6 rounded-3xl bg-white dark:bg-stone-850 border-2 border-stone-200 dark:border-stone-700 shadow-sm flex flex-col gap-4"
      >
        <div class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <span class="text-2xl font-black uppercase">{{ b.id }}</span>
            <span
              :class="[
                'px-3 py-1 rounded-full text-xs font-bold',
                b.healthy ? 'bg-emerald-100 text-emerald-800' : 'bg-stone-200 text-stone-600'
              ]"
            >
              {{ b.healthy ? '已偵測可運作' : '未安裝或異常' }}
            </span>
          </div>

          <button
            type="button"
            class="px-4 py-2 rounded-xl bg-stone-100 hover:bg-stone-200 dark:bg-stone-700 font-bold text-sm"
            @click="test(b.id)"
          >
            連線測試
          </button>
        </div>

        <div class="text-sm text-stone-500 font-mono">
          版本：{{ b.version || '未知' }}
        </div>

        <div class="flex flex-wrap gap-2 text-xs font-bold">
          <span class="px-2 py-1 rounded bg-stone-100 dark:bg-stone-800">
            視覺能力: {{ b.capabilities.vision }}
          </span>
          <span class="px-2 py-1 rounded bg-stone-100 dark:bg-stone-800">
            檔案讀寫: {{ b.capabilities.reads_files ? '讀取' : '' }} {{ b.capabilities.writes_files ? '寫入' : '' }}
          </span>
        </div>

        <!-- Toggle active & vision active -->
        <div class="flex items-center gap-6 pt-2 border-t border-stone-100 dark:border-stone-800">
          <label class="flex items-center gap-2 cursor-pointer font-bold text-sm">
            <input
              type="checkbox"
              :value="b.id"
              v-model="backends.active"
              class="w-4 h-4 accent-amber-600"
            />
            啟用為通用後端
          </label>

          <label
            v-if="b.capabilities.vision !== 'none'"
            class="flex items-center gap-2 cursor-pointer font-bold text-sm text-amber-700 dark:text-amber-400"
          >
            <input
              type="checkbox"
              :value="b.id"
              v-model="backends.activeVision"
              class="w-4 h-4 accent-amber-600"
            />
            啟用為看圖專用
          </label>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import * as api from './api';

const backends = ref<{
  available: any[];
  active: string[];
  activeVision: string[];
}>({
  available: [],
  active: [],
  activeVision: [],
});

onMounted(async () => {
  backends.value = await api.fetchBackends();
});

async function test(id: string) {
  try {
    const res = await api.testBackend(id);
    if (res.ok) {
      alert(`後端 ${id} 測試成功！延遲：${res.durationMs}ms\n回覆：${res.reply}`);
    } else {
      alert(`後端 ${id} 測試失敗：${res.error}`);
    }
  } catch (e: any) {
    alert(e.message || '測試失敗');
  }
}

async function saveBackends() {
  try {
    backends.value = await api.updateBackends({
      active: backends.value.active,
      activeVision: backends.value.activeVision,
    });
    alert('AI 後端設定已更新！');
  } catch (e: any) {
    alert(e.message || '儲存失敗');
  }
}
</script>
