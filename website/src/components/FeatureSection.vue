<script setup lang="ts">
import { Check } from "@lucide/vue";
import type { FeatureCopy } from "../content";
import FeatureVisual from "./FeatureVisual.vue";

defineProps<{
  feature: FeatureCopy;
  reverse: boolean;
}>();
</script>

<template>
  <article class="feature-row" :class="{ 'feature-row--reverse': reverse }">
    <div class="feature-copy">
      <h3>{{ feature.title }}</h3>
      <p class="feature-body">{{ feature.body }}</p>
      <ul>
        <li v-for="point in feature.points" :key="point"><Check :size="17" />{{ point }}</li>
      </ul>
    </div>
    <FeatureVisual :feature="feature" />
  </article>
</template>
