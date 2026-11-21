// Runs inside an opaque-origin sandbox. Only the transferred port can answer these calls.
const channel = new MessageChannel();
const requests = new Map();
let seq = 0;
let restore;
window.agentContextSavedDraft = new Promise((resolve) => {
  restore = resolve;
});
window.agentContextSaveDraft = (draft) => channel.port1.postMessage({ type: 'draft', draft });
channel.port1.onmessage = ({ data }) => {
  if (data.type === 'restore') {
    restore(data.draft);
    return;
  }
  const request = requests.get(data.id);
  if (!request) return;
  requests.delete(data.id);
  if (data.status >= 400) {
    const error = new Error(data.body?.error || '请求失败');
    error.status = data.status;
    request.reject(error);
  } else request.resolve(data.body);
};
window.agentContextApi = (path, method, body) =>
  new Promise((resolve, reject) => {
    const id = ++seq;
    requests.set(id, { resolve, reject });
    channel.port1.postMessage({ id, path, method, body });
  });
parent.postMessage({ agentContextFrame: document.querySelector('meta[name=bridge]').content }, '*', [
  channel.port2,
]);
