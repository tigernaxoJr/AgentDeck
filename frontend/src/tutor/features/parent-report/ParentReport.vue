<template>
  <div class="h-screen overflow-y-auto bg-[#faf6f0] dark:bg-[#1c1917] text-stone-900 dark:text-stone-100">
  <div class="p-8 max-w-5xl mx-auto flex flex-col gap-8">
    <div class="flex items-center justify-between pb-6 border-b-2 border-stone-200 dark:border-stone-800">
      <h1 class="text-4xl font-black text-amber-800 dark:text-amber-500">
        📊 學童複習成效與弱點診斷報告
      </h1>
      <router-link to="/tutor" class="px-5 py-2.5 rounded-xl bg-stone-200 dark:bg-stone-800 font-bold text-lg">
        返回首頁
      </router-link>
    </div>

    <!-- Loading / Overview -->
    <div v-if="loading" class="text-2xl font-bold py-12 text-center text-stone-500">
      載入報告中...
    </div>

    <div v-else-if="report" class="flex flex-col gap-8">
      <!-- 1. Due Count Card -->
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-6 shadow-md border-2 border-stone-200 dark:border-stone-700 flex items-center justify-between">
        <div>
          <span class="text-xl font-bold text-stone-500">目前間隔複習到期題數</span>
          <div class="text-5xl font-black text-amber-600 mt-2">{{ report.dueCount }} 題</div>
        </div>
        <router-link to="/tutor" class="px-6 py-4 rounded-2xl bg-amber-600 text-white font-bold text-xl hover:bg-amber-700">
          立即複習
        </router-link>
      </div>

      <!-- 2. Weak Concepts Ranking -->
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-6 shadow-md border-2 border-stone-200 dark:border-stone-700 flex flex-col gap-4">
        <h2 class="text-2xl font-black">🎯 概念弱點排行</h2>
        <div v-if="report.weakConcepts.length === 0" class="text-lg text-stone-500 py-4">
          目前沒有發現明顯的弱點概念，表現非常優秀！🎉
        </div>
        <div v-else class="flex flex-col gap-3">
          <div
            v-for="(c, idx) in report.weakConcepts"
            :key="c.tag"
            class="flex items-start justify-between p-4 rounded-2xl bg-stone-50 dark:bg-stone-900 border border-stone-200 dark:border-stone-700"
          >
            <div class="flex items-start gap-4">
              <span class="w-8 h-8 rounded-full bg-amber-100 text-amber-800 dark:bg-amber-900 dark:text-amber-200 font-black flex items-center justify-center">
                {{ idx + 1 }}
              </span>
              <div>
                <span class="text-xl font-bold">{{ c.tag }}</span>
                <p v-if="c.lastNote" class="text-base text-stone-600 dark:text-stone-400 mt-1">
                  💡 老師筆記：{{ c.lastNote }}
                </p>
              </div>
            </div>
            <div class="text-right">
              <span class="text-lg font-black text-rose-600">累計答錯 {{ c.wrongCount }} 次</span>
            </div>
          </div>
        </div>
      </div>

      <!-- 3. Recent Wrong Questions -->
      <div class="bg-white dark:bg-stone-800 rounded-3xl p-6 shadow-md border-2 border-stone-200 dark:border-stone-700 flex flex-col gap-4">
        <h2 class="text-2xl font-black">📝 近期錯題歷程與作答紀錄</h2>
        <div v-if="report.recentWrong.length === 0" class="text-lg text-stone-500 py-4">
          近期作答全部正確，沒有錯題紀錄！
        </div>
        <div v-else class="flex flex-col gap-6">
          <div
            v-for="item in report.recentWrong"
            :key="item.question.id"
            class="p-6 rounded-2xl border-2 border-stone-200 dark:border-stone-700 bg-stone-50 dark:bg-stone-900 flex flex-col gap-4"
          >
            <div class="text-xl font-bold">{{ item.question.stem }}</div>
            <div class="flex items-center gap-2 text-stone-500 font-medium">
              <span>題型：{{ item.question.qtype }}</span>
              <span v-if="item.question.bookPage">｜課本第 {{ item.question.bookPage }} 頁</span>
            </div>

            <!-- Attempts history -->
            <div class="flex flex-col gap-3 mt-2">
              <div
                v-for="(att, aIdx) in item.attempts"
                :key="aIdx"
                class="p-4 rounded-xl bg-white dark:bg-stone-800 border border-stone-200 dark:border-stone-700 flex flex-col md:flex-row items-start md:items-center justify-between gap-4"
              >
                <div>
                  <div class="font-bold text-lg">
                    第 {{ aIdx + 1 }} 次作答：
                    <span :class="att.verdict === 'correct' ? 'text-emerald-600' : 'text-rose-600'">
                      {{ att.verdict }}
                    </span>
                  </div>
                  <div v-if="att.inputText" class="text-base text-stone-600 dark:text-stone-300">
                    輸入內容：{{ att.inputText }}
                  </div>
                  <div v-if="att.recognizedText" class="text-base text-stone-600 dark:text-stone-300">
                    AI 辨識手寫：{{ att.recognizedText }}
                  </div>
                </div>

                <div v-if="att.inputImageUrl">
                  <img
                    :src="att.inputImageUrl"
                    class="h-24 rounded-lg border border-stone-300 object-contain"
                    alt="Handwriting attempt"
                  />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
  </div>
</template>

<script setup lang="ts">
import { ref, onMounted } from 'vue';
import { useRoute } from 'vue-router';
import * as api from './api';

const route = useRoute();
const studentId = parseInt(route.params.studentId as string);

const report = ref<api.StudentReportView | null>(null);
const loading = ref(true);

onMounted(async () => {
  try {
    report.value = await api.fetchStudentReport(studentId);
  } catch (e: any) {
    alert(e.message || '載入報告失敗');
  } finally {
    loading.value = false;
  }
});
</script>
