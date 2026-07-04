<script setup lang="ts">
// 关于页（mockup 2.13）：图标 + logotype + 版本
import Button from "@/components/Button.vue";
import AppIcon from "@/components/AppIcon.vue";
</script>

<template>
  <div class="about">
    <AppIcon :size="88" />
    <div class="logotype">Typex</div>
    <p class="meta">
      v0.1.0 · GPL-3.0 · typex.ink<br />
      你的声音只会发送到你自己配置的服务。
    </p>
    <div class="actions">
      <Button size="sm">检查更新</Button>
      <Button variant="ghost" size="sm">开源许可</Button>
    </div>
  </div>
</template>

<style scoped>
.about {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  text-align: center;
  gap: 10px;
  height: 100%;
  min-height: 380px;
}
/* logotype：Inter SemiBold 字距 -1%，五字母同色（04 §2.3） */
.logotype {
  font-size: 26px;
  font-weight: 600;
  letter-spacing: -0.01em;
  color: var(--text-1);
}
.meta {
  font-size: 12px;
  color: var(--text-2);
  line-height: 1.6;
}
.actions {
  display: flex;
  gap: 8px;
}
</style>
