'use strict';
const $ = id => document.getElementById(id);
const state = { area: 'board', selected: null, data: {board: [], wiki: [], raw: []}, doc: null, editing: false, fresh: false, boardOpen: false, busy: false, external: false, generation: 0, refreshing: false };
const escapeHtml = value => String(value ?? '').replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
let toastTimer;
let pointerDown = false, listHtml;
function readingSelection() { const selection = window.getSelection(); return pointerDown || !!selection && !selection.isCollapsed; }
function toast(message) { $('toast').textContent = message; $('toast').hidden = false; clearTimeout(toastTimer); toastTimer = setTimeout(() => $('toast').hidden = true, 6000); }
async function api(path, method = 'GET', body) {
  if (window.agentContextApi) return window.agentContextApi(path, method, body);
  const response = await fetch(path, {method, headers: {'Content-Type': 'application/json', 'X-Agent-Context': '1'}, ...(body === undefined ? {} : {body: JSON.stringify(body)})});
  const content = await response.text();
  let data; try { data = JSON.parse(content); } catch { data = {error: content}; }
  if (!response.ok) { const error = new Error(data.error || `请求失败 (${response.status})`); error.status = response.status; throw error; }
  return data;
}
function modal({title, message = '', content = '', confirm = '确认', cancel = '取消', onMount}) {
  if ($('dialog').open) return Promise.resolve(null);
  $('dialog-title').textContent = title; $('dialog-message').textContent = message;
  $('dialog-content').innerHTML = content; $('dialog-confirm').textContent = confirm; $('dialog-cancel').textContent = cancel;
  $('dialog').returnValue = ''; $('dialog').showModal(); onMount?.();
  return new Promise(resolve => $('dialog').addEventListener('close', () => {
    const values = Object.fromEntries([...$('dialog-content').querySelectorAll('[name]')].map(el => [el.name, el.value]));
    resolve($('dialog').returnValue === 'confirm' ? values : null);
  }, {once: true}));
}
function dirty() { return state.editing && (state.fresh || $('source').value !== (state.doc?.text || '')); }
async function leaveDraft() {
  if (!dirty()) return true;
  return !!await modal({title:'有尚未保存的修改', message:'离开会丢弃当前草稿。', confirm:'放弃修改', cancel:'继续编辑'});
}
let trackEmbeddedDraft = false;
function rememberDraft() {
  if (!trackEmbeddedDraft || !window.agentContextSaveDraft || !state.data.root) return;
  const draft = dirty() ? { root: state.data.root, area: state.area, selected: state.selected,
    fresh: state.fresh, doc: state.doc, key: $('name').value, text: $('source').value } : null;
  window.agentContextSaveDraft(draft);
}
function currentItem() { return state.data[state.area].find(row => (row.key || row.path) === state.selected); }
function fingerprint(row) { return !row ? null : state.area === 'board' ? JSON.stringify([row.version,row.value,row.updated_at]) : row.revision; }
function boardTitle(row) { return row.value.split('\n').find(s => s.trim())?.replace(/^#+\s*/, '').slice(0, 90) || row.key; }
function boardSummary(row) { const lines = row.value.trim().split('\n'); return lines.slice(1).filter(s => s.trim() && !s.startsWith('#')).join(' ').slice(0, 160); }
function time(value) { const d = new Date(value); return Number.isNaN(d.valueOf()) ? value : new Intl.DateTimeFormat('zh-CN', {month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',hour12:false}).format(d); }
function renderList() {
  const query = $('query').value.trim().toLowerCase();
  const rows = state.data[state.area].filter(row => `${row.key || row.path} ${row.value || row.search_text || ''} ${row.title || ''} ${(row.tags || []).join(' ')}`.toLowerCase().includes(query));
  $('list-count').textContent = `${rows.length} 条${query ? ' / 共 ' + state.data[state.area].length + ' 条' : ''}${state.area === 'board' ? ' · 最近更新' : ''}`;
  const grouped = new Map();
  for (const row of rows) { const group = state.area === 'board' ? row.key.split(':')[0] : state.area === 'raw' ? row.compiled ? '已整理' : '待整理' : '主题页面'; if (!grouped.has(group)) grouped.set(group, []); grouped.get(group).push(row); }
  const html = [...grouped].map(([group, list]) => `<div class="group">${escapeHtml({repo:'仓库交接 · repo',worktree:'工作区 · worktree',port:'端口 · port',env:'环境 · env'}[group] || group)} · ${list.length}</div>` + list.map(row => {
    const key = row.key || row.path, board = state.area === 'board';
    const chosen = state.selected === key && (!board || state.boardOpen);
    const tags = row.tags || [];
    const status = state.area === 'raw' ? `<span class="item-status">${row.compiled ? '已整理' : '待整理'}</span>` : '';
    const labels = !board && (tags.length || status) ? `<span class="item-labels">${status}${tags.slice(0, 2).map(tag => `<span class="item-tag">${escapeHtml(tag)}</span>`).join('')}${tags.length > 2 ? `<span class="tag-more" title="${escapeHtml(tags.slice(2).join('、'))}">+${tags.length - 2}</span>` : ''}</span>` : '';
    return `<button class="item${chosen ? ' selected' : ''}" data-item="${escapeHtml(key)}" aria-pressed="${chosen}" title="${escapeHtml(key)}"><span class="item-title">${escapeHtml(board ? boardTitle(row) : row.title)}</span>${board ? `<span class="item-key">${escapeHtml(key)}</span>` : ''}<span class="item-summary">${escapeHtml(board ? boardSummary(row) : row.summary)}</span>${labels}${board ? `<span class="item-meta"><span>${escapeHtml(time(row.updated_at))}</span><span>查看 →</span></span>` : ''}</button>`;
  }).join('')).join('') || `<div class="empty">${query ? '没有匹配的内容' : state.area === 'board' ? '黑板还是空的。新建一条记录，或让 agent 写下交接。' : '这里还没有内容。'}</div>`;
  if (html !== listHtml) { $('items').innerHTML = html; listHtml = html; }
}
function chrome() {
  rememberDraft();
  $('app').className = `${state.area} ${!state.boardOpen ? 'overview' : ''}`;
  document.querySelectorAll('[data-area]').forEach(button => button.setAttribute('aria-current', button.dataset.area === state.area ? 'page' : 'false'));
  for (const area of ['board','wiki','raw']) $(`count-${area}`).textContent = state.data[area].length;
  $('area-title').textContent = {board:'黑板',wiki:'Wiki',raw:'原始记录'}[state.area];
  $('subtitle').textContent = {board:'跨工具交接与本机工作状态',wiki:'按主题积累的长期知识',raw:'写入现场的原始记录 · 只读'}[state.area];
  $('new').hidden = state.area === 'raw'; $('query').placeholder = state.area === 'board' ? '搜索 key 或正文' : '搜索标题、标签或正文';
  $('back').hidden = !state.boardOpen; $('back').textContent = state.area === 'board' ? '← 返回总览' : '← 返回列表'; $('reader').hidden = state.editing; $('editor').hidden = !state.editing;
  $('edit').hidden = state.area === 'raw' || (!state.doc && !state.editing); $('edit').textContent = state.editing ? '保存' : '编辑';
  $('cancel').hidden = !state.editing; $('delete').hidden = state.area !== 'board' || !state.doc || state.editing;
  $('commit').hidden = state.area !== 'wiki' || !state.doc || state.editing;
  $('external').hidden = !state.external;
  $('location').textContent = state.editing && state.fresh ? '新建记录' : state.selected || '';
  $('meta').textContent = state.area === 'board' ? `本机黑板 · 不进入 Git${state.doc ? ' · v' + state.doc.version + ' · ' + time(state.doc.updated_at) : ''}` : state.area === 'raw' ? '原始记录只追加，保留作为来源' : '保存到 Markdown · Git 提交单独操作';
  for (const id of ['edit','cancel','delete','commit','new']) $(id).disabled = state.busy;
}
function linkify(container) {
  const walker = document.createTreeWalker(container, NodeFilter.SHOW_TEXT);
  const nodes = []; while (walker.nextNode()) if (!walker.currentNode.parentElement.closest('a,code,pre')) nodes.push(walker.currentNode);
  for (const node of nodes) {
    const matches = [...node.textContent.matchAll(/\[\[([^\]]+)\]\]/g)]; if (!matches.length) continue;
    const fragment = document.createDocumentFragment(); let at = 0;
    for (const match of matches) {
      fragment.append(node.textContent.slice(at, match.index)); const [target,label] = match[1].split('|'); const a = document.createElement('a');
      let path = target.split('#')[0]; if (!path.startsWith('wiki/') && !path.startsWith('raw/')) path = 'wiki/' + path;
      if (!path.endsWith('.md')) path += '.md'; a.href = '/?page=' + encodeURIComponent(path); a.dataset.page = path; a.textContent = label || target; fragment.append(a); at = match.index + match[0].length;
    }
    fragment.append(node.textContent.slice(at)); node.replaceWith(fragment);
  }
  container.querySelectorAll('a[href]').forEach(link => {
    if (link.dataset.page) return;
    const href = link.getAttribute('href');
    if (href.startsWith('#')) return;
    if (/^https?:\/\//i.test(href)) { link.target = '_blank'; link.rel = 'noopener noreferrer'; return; }
    if (!href || /^[a-z]+:/i.test(href)) return;
    try {
      const base = new URL(state.selected || 'wiki/index.md', (location.origin === 'null' ? 'https://agent-context.invalid' : location.origin) + '/');
      const url = new URL(href, base); const path = decodeURIComponent(url.pathname.slice(1));
      if (path.endsWith('.md') && /^(wiki|raw)\//.test(path)) { link.dataset.page = path; link.href = '/?page=' + encodeURIComponent(path); }
    } catch { /* Keep malformed Markdown links inert. */ link.removeAttribute('href'); }
  });
}
async function getDocument(area, id) {
  if (area === 'board') {
    const row = state.data.board.find(r => r.key === id); if (!row) return null;
    const rendered = await api('/api/preview','POST',{text:row.value});
    return {...row,text:row.value,html:rendered.html};
  }
  return api('/api/document?path=' + encodeURIComponent(id));
}
function showReader() {
  $('reader').innerHTML = state.doc?.html || '<div class="empty">选择一条记录开始阅读</div>';
  if (state.area === 'raw' && state.doc) {
    const name = state.selected.replace(/\.md$/, '');
    const sources = state.data.wiki.filter(page => page.search_text.includes(name));
    if (sources.length) $('reader').insertAdjacentHTML('beforeend', '<h2>引用此记录的页面</h2>' + sources.map(page => `<p><a href="/?page=${encodeURIComponent(page.path)}" data-page="${escapeHtml(page.path)}">${escapeHtml(page.title)}</a></p>`).join(''));
  }
  linkify($('reader')); chrome();
}
async function select(id) {
  if (state.busy || !await leaveDraft()) return;
  state.editing = false; state.fresh = false; state.external = false; state.selected = id; state.boardOpen = true;
  const gen = ++state.generation, area = state.area; state.doc = null; chrome(); renderList(); $('detail').scrollTop = 0; $('reader').textContent = '正在读取…';
  try { const doc = await getDocument(area,id); if (gen !== state.generation) return; state.doc = doc; showReader(); }
  catch (e) { if (gen === state.generation) { $('reader').textContent = '无法读取内容：' + e.message; chrome(); } }
}
async function switchArea(area) {
  if (state.busy || !await leaveDraft()) return;
  ++state.generation; state.area = area; state.selected = null; state.doc = null; state.editing = false; state.fresh = false; state.external = false; state.boardOpen = false;
  $('query').value = ''; chrome(); renderList(); $('items').scrollTop = 0; showReader();
  if (area !== 'board' && window.innerWidth > 760 && state.data[area].length) await select(state.data[area][0].path);
}
async function refresh(manual = false) {
  const paused = () => !manual && readingSelection();
  if (state.refreshing || state.busy || paused()) return;
  state.refreshing = true;
  try {
    const data = await api('/api/catalog');
    if (state.busy || paused()) return;
    // Compare against the displayed document so a deferred update is retried.
    const before = fingerprint(state.doc); state.data = data;
    $('vault-name').textContent = data.root; $('connection').textContent = '● 本机已连接';
    const after = fingerprint(currentItem()); chrome(); renderList();
    if (state.selected && before !== after && !state.fresh) {
      if (state.editing) { state.external = true; $('external-message').textContent = after ? '内容已有更新，当前草稿已保留。' : '这条内容已被删除，当前草稿已保留。'; chrome(); }
      else if (after) {
        const gen = ++state.generation; const doc = await getDocument(state.area,state.selected);
        if (gen === state.generation && !state.editing && !paused()) { state.doc = doc; showReader(); }
      } else { state.doc = null; state.selected = null; showReader(); }
    }
    if (manual) toast('内容已刷新');
  } catch (e) { $('connection').textContent = '连接中断 · 自动重试'; if (manual) toast(e.message); }
  finally { state.refreshing = false; }
}
function edit(fresh = false) {
  if (!fresh && !state.doc) return;
  state.editing = true; state.fresh = fresh; state.external = false; state.boardOpen = true;
  $('name-caption').textContent = state.area === 'board' ? '条目 key（用 : 分段）' : '页面路径（wiki/ 下的 .md 文件）';
  $('name').value = fresh ? state.area === 'board' ? '' : 'wiki/' : state.selected;
  $('name').readOnly = !fresh; $('source').value = fresh ? '# 新记录\n\n' : state.doc.text;
  $('source').hidden = false; $('preview-body').hidden = true; $('preview').textContent = '预览';
  $('save-hint').textContent = state.area === 'board' ? '保存时检查版本，避免覆盖其他会话的修改' : '保留完整 Markdown（含元数据）；保存后可单独提交 Git';
  $('dirty').textContent = fresh ? '未保存' : ''; chrome(); if (fresh) $('name').focus(); else $('source').focus();
}
async function newItem() { if (state.busy || !await leaveDraft()) return; ++state.generation; state.selected = null; state.doc = null; edit(true); renderList(); }
async function save() {
  if (state.busy) return;
  const key = $('name').value.trim(), text = $('source').value;
  if (!key || !text.trim()) return toast('请填写名称和正文');
  const area = state.area; state.busy = true; chrome();
  try {
    if (area === 'board') await api('/api/board','POST',{key,value:text,expect_version:state.fresh ? 0 : state.doc.version});
    else await api('/api/document','PUT',{path:key,text,expected_revision:state.fresh ? null : state.doc.revision});
    state.editing = false; state.fresh = false; state.external = false; state.selected = key;
    // Read the canonical saved text/version (board writes trim whitespace).
    state.data = await api('/api/catalog'); state.doc = await getDocument(area,key); ++state.generation;
    showReader(); renderList(); toast('已保存到本机文件');
  } catch (e) { if (e.status === 409) { state.external = true; $('external-message').textContent = '保存冲突：其他会话已修改内容，当前草稿已保留。'; } toast(e.message); }
  finally { state.busy = false; chrome(); }
}
async function showLatest() {
  try {
    const data = await api('/api/catalog'); state.data = data; renderList();
    const latest = await getDocument(state.area,state.selected || $('name').value.trim());
    if (!latest) return toast('内容已被删除，请复制草稿后新建记录');
    const result = await modal({title:'合并最新内容',message:'对照最新内容修改下方草稿。确认后更新编辑基准，仍需点击保存。',content:'<label>最新内容<textarea name="latest" readonly></textarea></label><label>你的草稿（可在此合并）<textarea name="merged"></textarea></label>',confirm:'使用合并草稿',cancel:'继续原草稿',onMount:()=>{document.querySelector('[name=latest]').value = latest.text; document.querySelector('[name=merged]').value = $('source').value;}});
    if (result) { state.doc = latest; state.fresh = false; state.external = false; state.selected = latest.key || latest.path; $('name').value = state.selected; $('name').readOnly = true; $('source').value = result.merged; $('source').hidden = false; $('preview-body').hidden = true; $('preview').textContent = '预览'; $('dirty').textContent = dirty() ? '未保存' : ''; chrome(); }
  } catch (e) { toast(e.message); }
}
async function remove() {
  const doc = state.doc; if (!doc || state.busy) return;
  if (!await modal({title:'删除黑板记录',message:`将删除 ${doc.key}。黑板没有 Git 历史，删除后无法在页面中撤销。`,confirm:'删除记录'})) return;
  state.busy = true; chrome();
  try { await api('/api/board','DELETE',{key:doc.key,expect_version:doc.version}); state.doc = null; state.selected = null; state.boardOpen = false; state.data = await api('/api/catalog'); renderList(); showReader(); toast('记录已删除'); }
  catch (e) { toast(e.message); }
  finally { state.busy = false; chrome(); }
}
async function commit() {
  if (state.busy) return;
  try {
    const info = await api('/api/git?path=' + encodeURIComponent(state.selected));
    if (!info.changed) return toast('当前页面没有待提交的修改');
    const result = await modal({title:'提交当前 Wiki 页面',message:`仅提交 ${state.selected}，其他文件保持原样。${info.new_file ? '这是新页面，下方展示完整内容。' : ''}`,content:`<pre>${escapeHtml(info.diff)}</pre><label>提交说明<input name="message" required placeholder="说明这次修改"></label>`,confirm:'提交到 Git'});
    if (!result) return; if (!result.message.trim()) return toast('请输入提交说明');
    state.busy = true; chrome();
    const outcome = await api('/api/git','POST',{path:info.path,expected_revision:info.revision,message:result.message});
    toast(outcome.committed ? `已提交 ${outcome.commit}` : '没有待提交的修改');
  } catch (e) { toast(e.message); }
  finally { state.busy = false; chrome(); }
}
// All persisted changes are explicit actions; polling never replaces a draft.
// Keep DOM nodes stable while dragging or copying, including requests already in flight.
window.addEventListener('pointerdown', () => { pointerDown = true; }, true);
window.addEventListener('pointerup', () => { pointerDown = false; }, true);
window.addEventListener('pointercancel', () => { pointerDown = false; }, true);
window.addEventListener('blur', () => { pointerDown = false; });
document.querySelectorAll('[data-area]').forEach(button => button.addEventListener('click',()=>switchArea(button.dataset.area)));
$('items').addEventListener('click',event => {const button=event.target.closest('[data-item]');if(button) select(button.dataset.item);});
$('detail').addEventListener('click',async event => {const link=event.target.closest('[data-page]');if(!link)return;event.preventDefault();const path=link.dataset.page;if(state.busy||!await leaveDraft())return;state.editing=false;state.area=path.startsWith('raw/')?'raw':'wiki';$('query').value='';await select(path);});
$('query').addEventListener('input',renderList); $('refresh').onclick=()=>refresh(true); $('new').onclick=newItem;
$('edit').onclick=()=>state.editing?save():edit(); $('cancel').onclick=async()=>{if(await leaveDraft()){state.editing=false;state.fresh=false;state.external=false;if(state.selected)await select(state.selected);else {state.boardOpen=false;showReader();}}};
$('back').onclick=async()=>{if(state.busy||!await leaveDraft())return;++state.generation;state.editing=false;state.fresh=false;state.external=false;state.boardOpen=false;chrome();renderList();};
$('delete').onclick=remove; $('commit').onclick=commit; $('latest').onclick=showLatest;
$('source').addEventListener('input',()=>{$('dirty').textContent=dirty()?'未保存':'';});
$('preview').onclick=async()=>{if(!$('preview-body').hidden){$('preview-body').hidden=true;$('source').hidden=false;$('preview').textContent='预览';return;}try{const result=await api('/api/preview','POST',{text:$('source').value});$('preview-body').innerHTML=result.html;linkify($('preview-body'));$('preview-body').hidden=false;$('source').hidden=true;$('preview').textContent='返回编辑';}catch(e){toast(e.message);}};
window.addEventListener('beforeunload',event=>{if(dirty()){event.preventDefault();event.returnValue='';}});
window.addEventListener('keydown',event=>{if((event.metaKey||event.ctrlKey)&&event.key==='s'&&state.editing){event.preventDefault();save();}});
if (window.agentContextApi) {
  document.addEventListener('input', rememberDraft);
  document.querySelector('.brand').onclick = async event => { event.preventDefault(); await switchArea('board'); };
}

(async () => {
  const draft = await window.agentContextSavedDraft;
  await refresh();
  if (draft && draft.root === state.data.root && ['board','wiki'].includes(draft.area)) {
    state.area = draft.area; state.selected = draft.selected; state.doc = draft.doc;
    edit(draft.fresh);
    $('name').value = draft.key; $('source').value = draft.text; $('dirty').textContent = '未保存';
    renderList(); await refresh(true);
  } else {
    const path = new URLSearchParams(location.search).get('page');
    if (path && /^(wiki|raw)\//.test(path)) { state.area = path.startsWith('raw/') ? 'raw' : 'wiki'; await select(path); }
    else { chrome(); renderList(); }
  }
  trackEmbeddedDraft = true;
  setInterval(() => refresh(), 3000);
})();
