window.__ModuleLoader__.load({
  id: '__PACKAGE_ID__',
  factory(require) {
    const React = require('react');
    const h = React.createElement;
    const source = '__FRAME_HTML__';
    const drafts = new Map();
    const base = new URL('api/agent-context/', document.baseURI);
    function Frame({ vault, title }) {
      const ref = React.useRef(null);
      const [doc, setDoc] = React.useState('');
      React.useEffect(() => {
        const token = crypto.randomUUID();
        let port;
        const controller = new AbortController();
        function connect(e) {
          if (
            port ||
            e.source !== ref.current?.contentWindow ||
            e.origin !== 'null' ||
            e.data?.agentContextFrame !== token ||
            !e.ports[0]
          )
            return;
          port = e.ports[0];
          port.onmessage = async ({ data }) => {
            if (data.type === 'draft') {
              if (data.draft) drafts.set(vault, data.draft);
              else drafts.delete(vault);
              return;
            }
            let status = 503,
              body;
            try {
              const res = await fetch(new URL('request', base), {
                method: 'POST',
                credentials: 'same-origin',
                signal: controller.signal,
                headers: { 'Content-Type': 'application/json', 'X-Agent-Context': '1' },
                body: JSON.stringify({ vault, path: data.path, method: data.method, body: data.body }),
              });
              status = res.status;
              body = await res.json();
            } catch (err) {
              status = 503;
              body = { error: err.message };
            }
            if (!controller.signal.aborted) port.postMessage({ id: data.id, status, body });
          };
          port.postMessage({ type: 'restore', draft: drafts.get(vault) ?? null });
        }
        window.addEventListener('message', connect);
        setDoc(source.replace('__BRIDGE_TOKEN__', token));
        return () => {
          window.removeEventListener('message', connect);
          controller.abort();
          port?.close();
        };
      }, [vault]);
      return (
        doc &&
        h('iframe', {
          ref,
          title,
          srcDoc: doc,
          sandbox: 'allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox',
          style: { border: 0, flex: 1, width: '100%', minHeight: 0 },
        })
      );
    }
    function Page({ t }) {
      const [vaults, setVaults] = React.useState(null),
        [selected, setSelected] = React.useState('');
      const [error, setError] = React.useState(''),
        [attempt, setAttempt] = React.useState(0);
      React.useEffect(() => {
        const c = new AbortController();
        fetch(new URL('vaults', base), { credentials: 'same-origin', cache: 'no-store', signal: c.signal })
          .then(async (res) => {
            if (!res.ok) throw Error(String(res.status));
            return res.json();
          })
          .then((names) => {
            if (!c.signal.aborted) {
              setVaults(names);
              setSelected(names[0] || '');
              setError('');
            }
          })
          .catch((err) => {
            if (!c.signal.aborted) setError(err.message);
          });
        return () => c.abort();
      }, [attempt]);
      return h(
        'section',
        { style: { display: 'flex', flexDirection: 'column', height: '100%', minHeight: 0 } },
        vaults?.length > 1 &&
          h(
            'select',
            { 'aria-label': t('vault'), value: selected, onChange: (e) => setSelected(e.target.value) },
            vaults.map((n) => h('option', { key: n }, n)),
          ),
        selected
          ? h(Frame, { key: selected, vault: selected, title: t('title') })
          : h(
              'div',
              { role: 'status' },
              error || t(vaults ? 'empty' : 'waiting'),
              ' ',
              h('button', { onClick: () => setAttempt((i) => i + 1) }, t('refresh')),
            ),
      );
    }
    function Icon({ size }) {
      return h(
        'svg',
        {
          viewBox: '0 0 24 24',
          width: size,
          height: size,
          fill: 'none',
          stroke: 'currentColor',
          strokeWidth: 1.7,
          'aria-hidden': true,
        },
        h('path', { d: 'M12 6c-2-2-6-2-9-1v14c3-1 7-1 9 1 2-2 6-2 9-1V5c-3-1-7-1-9 1Zm0 0v14' }),
      );
    }
    return {
      inject: ['locale', 'slots'],
      apply(ctx) {
        ctx.effect(() => () => drafts.clear());
        const ns = 'agentContextPage',
          key = 'agent-context';
        ctx.effect(() =>
          ctx.locale.register(ns, {
            zh: {
              title: 'Agent Context',
              vault: '知识库',
              waiting: '正在读取知识库',
              empty: '没有已启用的知识库',
              refresh: '刷新',
            },
            en: {
              title: 'Agent Context',
              vault: 'Vault',
              waiting: 'Loading vaults',
              empty: 'No enabled vaults',
              refresh: 'Refresh',
            },
          }),
        );
        const t = ctx.locale.bind(ns);
        ctx.slots.inject('main', () => ctx.slots.register({ name: 'main', key, locale: ns }, Page));
        ctx.slots.inject('sidebar.panellist', () =>
          ctx.slots.register(
            { name: 'sidebar.panellist', id: key, order: 20, label: () => t('title'), locale: ns },
            Icon,
          ),
        );
      },
    };
  },
});
