import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import Select from "./Select.vue";

const OPTIONS = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "亮色" },
  { value: "dark", label: "暗色" },
];

function mountSelect(value = "system") {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const onUpdate = vi.fn();
  const wrapper = mount(Select, {
    attachTo: host,
    props: {
      modelValue: value,
      "onUpdate:modelValue": onUpdate,
      options: OPTIONS,
    },
  });
  return { wrapper, onUpdate, host };
}

afterEach(() => {
  document.body.innerHTML = "";
});

describe("Select", () => {
  it("使用自绘菜单而不是原生 select", async () => {
    const { wrapper, onUpdate } = mountSelect();

    expect(wrapper.find("select").exists()).toBe(false);
    await wrapper.find("button.select").trigger("click");

    const menu = document.body.querySelector(".select-menu");
    expect(menu?.textContent).toContain("暗色");

    const dark = [...document.body.querySelectorAll<HTMLButtonElement>(".select-option")]
      .find((item) => item.textContent?.includes("暗色"))!;
    dark.click();
    await flushPromises();

    expect(onUpdate).toHaveBeenCalledWith("dark");
    expect(document.body.querySelector(".select-menu")).toBeNull();
  });

  it("支持键盘打开和选择", async () => {
    const { wrapper, onUpdate } = mountSelect();
    const trigger = wrapper.find("button.select");

    await trigger.trigger("keydown", { key: "ArrowDown" });
    await trigger.trigger("keydown", { key: "ArrowDown" });
    await document.body.querySelector<HTMLElement>(".select-menu")!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
    );

    expect(onUpdate).toHaveBeenCalledWith("light");
  });
});
