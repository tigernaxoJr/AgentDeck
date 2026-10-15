import { createRouter, createWebHistory } from 'vue-router';
import AntigravityConsole from './components/AntigravityConsole.vue';
import StartView from './tutor/features/start/StartView.vue';
import TutorSession from './tutor/features/tutor-session/TutorSession.vue';
import ParentReport from './tutor/features/parent-report/ParentReport.vue';
import AdminView from './tutor/features/admin/AdminView.vue';

const routes = [
  { path: '/', redirect: '/tutor' },
  { path: '/console', component: AntigravityConsole },
  { path: '/tutor', component: StartView },
  { path: '/tutor/session/:id', component: TutorSession },
  { path: '/tutor/report/:studentId', component: ParentReport },
  { path: '/tutor/admin', component: AdminView },
];

export const router = createRouter({
  history: createWebHistory(),
  routes,
});
