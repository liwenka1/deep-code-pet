// 历史对话窗口（history）：列出已有会话，点一条切过去。
// 数据来自 Rust 的 list_conversations（本身从 sessions/*.json 里读）。

const tauri = window.__TAURI__;
const el = {
  list: document.getElementById("list"),
  empty: document.getElementById("empty"),
  new: document.getElementById("new"),
  close: document.getElementById("close"),
};

let currentId = null;

function relative(updatedAtMs) {
  const minutes = Math.max(0, Math.floor((Date.now() - updatedAtMs) / 60000));
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)} 小时前`;
  return `${Math.floor(minutes / 1440)} 天前`;
}

async function refresh() {
  if (!tauri) return;
  const conversations = await tauri.core.invoke("list_conversations", { limit: 50 }).catch(() => []);
  currentId = await tauri.core.invoke("get_current_session").catch(() => null);

  el.list.replaceChildren();
  el.empty.hidden = conversations.length > 0;

  for (const conversation of conversations) {
    const row = document.createElement("div");
    row.className = "row" + (conversation.id === currentId ? " current" : "");
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", String(conversation.id === currentId));

    const title = document.createElement("span");
    title.className = "title";
    title.textContent = conversation.title;

    const meta = document.createElement("span");
    meta.className = "meta";
    meta.textContent = relative(conversation.updated_at_ms);

    row.append(title, meta);

    if (conversation.id === currentId) {
      const badge = document.createElement("span");
      badge.className = "badge";
      badge.textContent = "当前";
      row.insertBefore(badge, meta);
    }

    row.addEventListener("click", async () => {
      try {
        // 切过去：Rust 侧写 current-session.json 并广播 conversation-changed，
        // 聊天窗口会重读记录；这里自己收起。
        await tauri.core.invoke("open_conversation", { id: conversation.id });
        await tauri.core.invoke("close_history").catch(() => {});
      } catch (error) {
        console.error("[history] open failed", error);
      }
    });

    el.list.append(row);
  }
}

async function startNew() {
  await tauri?.core.invoke("new_conversation").catch(() => {});
  await tauri?.core.invoke("close_history").catch(() => {});
}

async function closePanel() {
  await tauri?.core.invoke("close_history").catch(() => {});
}

el.new.addEventListener("click", startNew);
el.close.addEventListener("click", closePanel);
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") closePanel();
});
window.addEventListener("focus", refresh);

// 启动 + 每次被叫出来都刷新（新对话会出现在最上面）
refresh();
