<template>
  <button
    :class="[
      'px-6 py-4 rounded-2xl font-bold text-2xl transition-all shadow-md active:scale-95 disabled:opacity-50 disabled:pointer-events-none cursor-pointer flex items-center justify-center gap-3',
      variantClasses
    ]"
    :disabled="disabled"
    @click="$emit('click', $event)"
  >
    <slot />
  </button>
</template>

<script setup lang="ts">
import { computed } from 'vue';

const props = withDefaults(
  defineProps<{
    variant?: 'primary' | 'secondary' | 'outline' | 'danger';
    disabled?: boolean;
  }>(),
  {
    variant: 'primary',
    disabled: false,
  }
);

defineEmits<{
  (e: 'click', event: MouseEvent): void;
}>();

const variantClasses = computed(() => {
  switch (props.variant) {
    case 'primary':
      return 'bg-amber-600 hover:bg-amber-700 text-white shadow-amber-900/20';
    case 'secondary':
      return 'bg-stone-200 hover:bg-stone-300 text-stone-800 dark:bg-stone-700 dark:hover:bg-stone-600 dark:text-stone-100';
    case 'outline':
      return 'border-3 border-stone-300 hover:border-amber-500 text-stone-700 dark:border-stone-600 dark:text-stone-200';
    case 'danger':
      return 'bg-rose-600 hover:bg-rose-700 text-white shadow-rose-900/20';
    default:
      return '';
  }
});
</script>
