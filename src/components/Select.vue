<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, ref } from "vue";

export interface Option {
  value: string;
  label: string;
}
const model = defineModel<string>({ required: true });
const props = defineProps<{ options: Option[]; disabled?: boolean }>();

const root = ref<HTMLElement | null>(null);
const trigger = ref<HTMLButtonElement | null>(null);
const menu = ref<HTMLElement | null>(null);
const open = ref(false);
const activeIndex = ref(0);
const menuStyle = ref<Record<string, string>>({});
const menuId = `select-${Math.random().toString(36).slice(2)}`;

const selectedIndex = computed(() => {
  const idx = props.options.findIndex((o) => o.value === model.value);
  return idx >= 0 ? idx : 0;
});
const selectedLabel = computed(() => props.options[selectedIndex.value]?.label ?? "");

function updateMenuPosition() {
  const el = trigger.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const gap = 4;
  const maxHeight = 240;
  const spaceBelow = window.innerHeight - rect.bottom - gap;
  const spaceAbove = rect.top - gap;
  const openUp = spaceBelow < Math.min(maxHeight, props.options.length * 34 + 8) && spaceAbove > spaceBelow;
  const width = Math.max(rect.width, 150);
  const left = Math.min(Math.max(rect.left, 8), Math.max(window.innerWidth - width - 8, 8));
  const availableHeight = Math.max(80, Math.min(maxHeight, openUp ? spaceAbove : spaceBelow));
  menuStyle.value = {
    left: `${left}px`,
    width: `${width}px`,
    maxHeight: `${availableHeight}px`,
    ...(openUp
      ? { bottom: `${window.innerHeight - rect.top + gap}px` }
      : { top: `${rect.bottom + gap}px` }),
  };
}

function addGlobalListeners() {
  document.addEventListener("pointerdown", onGlobalPointerDown, true);
  window.addEventListener("resize", updateMenuPosition);
  window.addEventListener("scroll", updateMenuPosition, true);
}

function removeGlobalListeners() {
  document.removeEventListener("pointerdown", onGlobalPointerDown, true);
  window.removeEventListener("resize", updateMenuPosition);
  window.removeEventListener("scroll", updateMenuPosition, true);
}

async function openMenu() {
  if (props.disabled || !props.options.length) return;
  activeIndex.value = selectedIndex.value;
  open.value = true;
  await nextTick();
  if (!open.value) return;
  updateMenuPosition();
  addGlobalListeners();
  menu.value?.focus();
}

function closeMenu(focusTrigger = false) {
  if (!open.value) return;
  open.value = false;
  removeGlobalListeners();
  if (focusTrigger) trigger.value?.focus();
}

function toggleMenu() {
  if (open.value) {
    closeMenu();
  } else {
    void openMenu();
  }
}

function choose(option: Option) {
  model.value = option.value;
  closeMenu(true);
}

function onGlobalPointerDown(event: PointerEvent) {
  const target = event.target as Node | null;
  if (!target) return;
  if (root.value?.contains(target) || menu.value?.contains(target)) return;
  closeMenu();
}

function moveActive(delta: number) {
  if (!props.options.length) return;
  activeIndex.value = (activeIndex.value + delta + props.options.length) % props.options.length;
  nextTick(() => {
    const active = menu.value?.querySelector<HTMLElement>(`[data-index="${activeIndex.value}"]`);
    if (typeof active?.scrollIntoView === "function") {
      active.scrollIntoView({ block: "nearest" });
    }
  });
}

function onKeydown(event: KeyboardEvent) {
  if (props.disabled) return;
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      if (!open.value) void openMenu();
      else moveActive(1);
      break;
    case "ArrowUp":
      event.preventDefault();
      if (!open.value) void openMenu();
      else moveActive(-1);
      break;
    case "Enter":
    case " ":
      event.preventDefault();
      if (!open.value) {
        void openMenu();
      } else if (props.options[activeIndex.value]) {
        choose(props.options[activeIndex.value]);
      }
      break;
    case "Escape":
      event.preventDefault();
      closeMenu(true);
      break;
    case "Tab":
      closeMenu();
      break;
  }
}

onBeforeUnmount(removeGlobalListeners);
</script>

<template>
  <span ref="root" class="select-wrap" :class="{ disabled }">
    <button
      ref="trigger"
      type="button"
      class="select"
      :disabled="disabled"
      aria-haspopup="listbox"
      :aria-expanded="open"
      :aria-controls="open ? menuId : undefined"
      @click="toggleMenu"
      @keydown="onKeydown"
    >
      <span class="label">{{ selectedLabel }}</span>
      <span class="arr">{{ open ? "▴" : "▾" }}</span>
    </button>
    <Teleport to="body">
      <div
        v-if="open"
        :id="menuId"
        ref="menu"
        class="select-menu"
        :style="menuStyle"
        role="listbox"
        tabindex="-1"
        @keydown="onKeydown"
      >
        <button
          v-for="(o, i) in options"
          :key="o.value"
          type="button"
          class="select-option"
          :class="{ selected: o.value === model, active: i === activeIndex }"
          role="option"
          :aria-selected="o.value === model"
          :data-index="i"
          @mouseenter="activeIndex = i"
          @click="choose(o)"
        >
          <span class="check">{{ o.value === model ? "✓" : "" }}</span>
          <span class="option-label">{{ o.label }}</span>
        </button>
      </div>
    </Teleport>
  </span>
</template>

<style scoped>
.select-wrap {
  position: relative;
  display: inline-flex;
  align-items: center;
  min-width: 150px;
}
.select {
  display: inline-flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  width: 100%;
  height: 32px;
  padding: 0 10px;
  border-radius: var(--radius-control);
  border: 1px solid var(--border);
  background: var(--surface-2);
  color: var(--text-1);
  font-size: 13px;
  font-family: inherit;
  cursor: pointer;
}
.select:hover:not(:disabled) {
  border-color: var(--border-2);
}
.select:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: 2px;
}
.label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.arr {
  color: var(--text-3);
  font-size: 10px;
  flex-shrink: 0;
}
.select:disabled {
  opacity: 0.45;
  cursor: default;
}
.select-menu {
  position: fixed;
  z-index: 1000;
  overflow-y: auto;
  padding: 5px;
  border: 1px solid var(--border-2);
  border-radius: var(--radius-card);
  background: var(--surface);
  box-shadow: var(--shadow);
}
.select-menu:focus {
  outline: none;
}
.select-option {
  width: 100%;
  min-height: 30px;
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 0 9px;
  border: none;
  border-radius: var(--radius-control);
  background: transparent;
  color: var(--text-1);
  font-family: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.select-option.active,
.select-option:hover {
  background: var(--sel-bg);
}
.select-option.selected {
  font-weight: 600;
}
.check {
  width: 12px;
  flex-shrink: 0;
  color: var(--text-1);
}
.option-label {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
