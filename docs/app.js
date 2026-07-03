/* molde docs — single-page engine.
   No dependencies. Renders the markdown embedded in content.js, provides
   hash routing, grouped navigation, Ctrl+K search, right-hand TOC with
   scroll-spy, copy buttons and theme switching. */

(function () {
  'use strict';

  const DOCS = window.DOCS || [];
  const byId = new Map(DOCS.map((d) => [d.id, d]));
  const FILE_TO_ID = new Map(DOCS.map((d) => [d.file.split('/').pop().toLowerCase(), d.id]));
  const GITHUB = 'https://github.com/MAWESI-SAS/molde/blob/main/';

  /* --------------------------------------------------------------- language */
  let LANG = 'en';
  try {
    LANG =
      localStorage.getItem('molde-docs-lang') ||
      ((navigator.language || '').toLowerCase().startsWith('es') ? 'es' : 'en');
  } catch (e) {
    /* private mode */
  }

  const STR = {
    en: {
      onThisPage: 'On this page',
      searchPlaceholder: 'Search commands, syntax, guides…',
      searchTrigger: 'Search docs…',
      prev: '← Previous',
      next: 'Next →',
      navigate: 'navigate',
      open: 'open',
      results: (n) => `${n} result${n > 1 ? 's' : ''}`,
      empty: 'No matches — try a command name (<b>apply</b>, <b>pull</b>) or a concept (<b>checks</b>, <b>drift</b>)',
      fallback: 'This page is not translated yet — showing the English source.',
    },
    es: {
      onThisPage: 'En esta página',
      searchPlaceholder: 'Busca comandos, sintaxis, guías…',
      searchTrigger: 'Buscar en docs…',
      prev: '← Anterior',
      next: 'Siguiente →',
      navigate: 'navegar',
      open: 'abrir',
      results: (n) => `${n} resultado${n > 1 ? 's' : ''}`,
      empty: 'Sin coincidencias — prueba un comando (<b>apply</b>, <b>pull</b>) o un concepto (<b>checks</b>, <b>drift</b>)',
      fallback: 'Esta página aún no está traducida — se muestra la fuente en inglés.',
    },
  };
  const t = (key, ...args) => {
    const v = STR[LANG][key];
    return typeof v === 'function' ? v(...args) : v;
  };
  const docTitle = (d) => (LANG === 'es' && d.title_es ? d.title_es : d.title);
  const docGroup = (d) => (LANG === 'es' && d.group_es ? d.group_es : d.group);
  const docMd = (d) => (LANG === 'es' && d.markdown_es ? d.markdown_es : d.markdown);
  const docIsFallback = (d) => LANG === 'es' && !d.markdown_es;

  /* ------------------------------------------------------------------ utils */
  const esc = (s) =>
    s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

  function slugify(text, used) {
    let slug = text
      .toLowerCase()
      .replace(/`/g, '')
      .replace(/[^a-z0-9À-ɏ\s-]/g, '')
      .trim()
      .replace(/\s+/g, '-');
    if (!slug) slug = 'section';
    let unique = slug;
    let n = 2;
    while (used.has(unique)) unique = `${slug}-${n++}`;
    used.add(unique);
    return unique;
  }

  /* ------------------------------------------------- syntax highlighting */
  function highlight(code, lang) {
    let html = esc(code);
    const rules = [];
    if (/^(bash|sh|shell|cmd|powershell)$/.test(lang)) {
      rules.push(
        [/(^|\n)(\s*#[^\n]*)/g, (m, a, b) => a + tok(b, 'com')],
        [/(&quot;.*?&quot;|&#39;.*?&#39;|'[^']*'|"[^"]*")/g, (m) => tok(m, 'str')],
        [/(^|\n)(\$ |&gt; )/g, (m, a, b) => a + tok(b, 'key')],
        [/(\s)(--?[a-zA-Z][\w-]*)/g, (m, a, b) => a + tok(b, 'flag')],
        [/\b(molde|cargo|curl|docker|git|npm|npx|code|dotnet|psql|pg_dump|sudo|sh|iwr|iex)\b/g, (m) => tok(m, 'key')]
      );
    } else if (/^(sql)$/.test(lang)) {
      rules.push(
        [/(--[^\n]*)/g, (m) => tok(m, 'com')],
        [/('[^']*')/g, (m) => tok(m, 'str')],
        [/\b(SELECT|FROM|WHERE|INSERT|INTO|UPDATE|DELETE|CREATE|DROP|ALTER|TABLE|INDEX|VIEW|FUNCTION|TRIGGER|CONSTRAINT|PRIMARY|FOREIGN|KEY|REFERENCES|NOT|NULL|DEFAULT|UNIQUE|CHECK|ON|CASCADE|SET|VALUES|AND|OR|ORDER|BY|GROUP|LIMIT|JOIN|LEFT|RIGHT|INNER|AS|BEGIN|COMMIT|ROLLBACK)\b/gi, (m) => tok(m, 'key')],
        [/\b(\d+)\b/g, (m) => tok(m, 'num')]
      );
    } else if (/^(yaml|yml|ini|toml|gitattributes|json)$/.test(lang)) {
      rules.push(
        [/(^|\n)(\s*#[^\n]*)/g, (m, a, b) => a + tok(b, 'com')],
        [/(&quot;.*?&quot;|'[^']*')/g, (m) => tok(m, 'str')],
        [/(^|\n)(\s*[\w.-]+)(\s*[:=])/g, (m, a, b, c) => a + tok(b, 'key') + c],
        [/\b(true|false|\d+)\b/g, (m) => tok(m, 'num')]
      );
    } else {
      // molde .model / ebnf / plain: keys, strings, comments
      rules.push(
        [/(^|\n)(\s*#[^\n]*)/g, (m, a, b) => a + tok(b, 'com')],
        [/(&quot;.*?&quot;)/g, (m) => tok(m, 'str')],
        [/(^|\n)(\s*)([\w-]+)(:)/g, (m, a, sp, k, c) => a + sp + tok(k, 'key') + c],
        [/\b(pk|identity|unique|maxlen|precision|default|dbtype|clr|enum|owns|as)\b/g, (m) => tok(m, 'flag')]
      );
    }
    for (const [re, fn] of rules) html = html.replace(re, fn);
    return html;

    function tok(text, kind) {
      return `span class="tok-${kind}"${text}/span`;
    }
  }
  const untok = (s) => s.replace(//g, '<').replace(//g, '>');

  /* ------------------------------------------------------ inline markdown */
  function inline(text, codes) {
    let s = text;
    // inline code first — protect from other transforms
    s = s.replace(/`([^`\n]+)`/g, (m, c) => stash(codes, `<code>${esc(c)}</code>`));
    s = esc(s);
    // images (rare) → plain links
    s = s.replace(/!\[([^\]]*)\]\(([^)\s]+)\)/g, '$1');
    // links
    s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (m, label, href) => {
      return `<a href="${rewriteHref(href)}"${/^https?:/.test(href) ? ' target="_blank" rel="noopener"' : ''}>${label}</a>`;
    });
    // autolinks that were escaped: &lt;https://…&gt;
    s = s.replace(/&lt;(https?:\/\/[^&\s]+)&gt;/g, '<a href="$1" target="_blank" rel="noopener">$1</a>');
    // bold / italic / strikethrough
    s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    s = s.replace(/(^|[\s(])\*([^*\n]+)\*(?=[\s).,;:!?]|$)/g, '$1<em>$2</em>');
    s = s.replace(/~~([^~]+)~~/g, '<del>$1</del>');
    return s;
  }

  function stash(codes, html) {
    codes.push(html);
    return `${codes.length - 1}`;
  }
  const unstash = (s, codes) => s.replace(/(\d+)/g, (m, i) => codes[+i]);

  function rewriteHref(href) {
    if (/^(https?:|mailto:)/.test(href)) return href;
    if (href.startsWith('#')) {
      return `#/${state.docId}/${href.slice(1)}`;
    }
    const [pathPart, frag] = href.split('#');
    const base = pathPart.split('/').pop().toLowerCase();
    if (FILE_TO_ID.has(base)) {
      const id = FILE_TO_ID.get(base);
      return frag ? `#/${id}/${frag}` : `#/${id}`;
    }
    // Not part of the site (examples/, source files…) → GitHub.
    return GITHUB + pathPart.replace(/^(\.\.\/)+|^\.\//g, '');
  }

  /* -------------------------------------------------------- block markdown */
  function render(markdown) {
    const codes = [];
    const used = new Set();
    const headings = [];
    let src = markdown.replace(/\r\n/g, '\n');

    // fenced code blocks
    src = src.replace(/```([\w-]*)\n([\s\S]*?)```/g, (m, lang, body) => {
      const clean = body.replace(/\n$/, '');
      const html =
        `<pre><span class="code-lang">${esc(lang)}</span>` +
        `<button class="code-copy" type="button" aria-label="Copy code">` +
        `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>` +
        `</button><code>${untok(highlight(clean, lang.toLowerCase()))}</code></pre>`;
      return stash(codes, html);
    });

    const lines = src.split('\n');
    const out = [];
    let para = [];
    let listStack = []; // {type:'ul'|'ol', indent}
    let quote = [];

    const flushPara = () => {
      if (para.length) {
        out.push(`<p>${inline(para.join(' '), codes)}</p>`);
        para = [];
      }
    };
    const closeLists = (toIndent = -1) => {
      while (listStack.length && listStack[listStack.length - 1].indent >= toIndent + 1) {
        out.push(`</${listStack.pop().type}>`);
      }
    };
    const flushQuote = () => {
      if (quote.length) {
        out.push(`<blockquote>${render(quote.join('\n')).html}</blockquote>`);
        quote = [];
      }
    };

    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];

      // blockquote
      const q = line.match(/^\s*>\s?(.*)$/);
      if (q) {
        flushPara();
        closeLists();
        quote.push(q[1]);
        continue;
      }
      flushQuote();

      // stashed code block on its own line
      if (/^\d+\s*$/.test(line)) {
        flushPara();
        closeLists();
        out.push(unstash(line.trim(), codes));
        continue;
      }

      // heading
      const h = line.match(/^(#{1,4})\s+(.*)$/);
      if (h) {
        flushPara();
        closeLists();
        const lvl = h[1].length;
        const raw = h[2].replace(/\s+#*\s*$/, '');
        const slug = slugify(raw.replace(/[*_`]/g, ''), used);
        if (lvl >= 2 && lvl <= 3) headings.push({ lvl, slug, text: raw.replace(/[*_`]/g, '') });
        out.push(
          `<h${lvl} id="${slug}">${inline(raw, codes)}<a class="heading-anchor" href="#/${state.docId}/${slug}" aria-label="Link to section">#</a></h${lvl}>`
        );
        continue;
      }

      // hr
      if (/^\s*(-{3,}|\*{3,})\s*$/.test(line)) {
        flushPara();
        closeLists();
        out.push('<hr>');
        continue;
      }

      // table
      if (/^\s*\|.*\|\s*$/.test(line) && i + 1 < lines.length && /^\s*\|[\s:|-]+\|\s*$/.test(lines[i + 1])) {
        flushPara();
        closeLists();
        const cells = (l) => l.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
        const head = cells(line);
        let rows = [];
        i += 2;
        while (i < lines.length && /^\s*\|.*\|\s*$/.test(lines[i])) {
          rows.push(cells(lines[i]));
          i++;
        }
        i--;
        let t = '<table><thead><tr>' + head.map((c) => `<th>${inline(c, codes)}</th>`).join('') + '</tr></thead><tbody>';
        for (const r of rows) t += '<tr>' + r.map((c) => `<td>${inline(c, codes)}</td>`).join('') + '</tr>';
        out.push(t + '</tbody></table>');
        continue;
      }

      // list item
      const li = line.match(/^(\s*)([-*+]|\d+\.)\s+(.*)$/);
      if (li) {
        flushPara();
        const indent = Math.floor(li[1].length / 2);
        const type = /^\d+\.$/.test(li[2]) ? 'ol' : 'ul';
        while (listStack.length && listStack[listStack.length - 1].indent > indent) {
          out.push(`</${listStack.pop().type}>`);
        }
        const top = listStack[listStack.length - 1];
        if (!top || top.indent < indent || top.type !== type) {
          if (top && top.indent === indent && top.type !== type) out.push(`</${listStack.pop().type}>`);
          listStack.push({ type, indent });
          out.push(`<${type}>`);
        }
        out.push(`<li>${inline(li[3], codes)}</li>`);
        continue;
      }

      // list continuation (indented text under an item)
      if (listStack.length && /^\s{2,}\S/.test(line)) {
        const last = out.length - 1;
        if (out[last] && out[last].endsWith('</li>')) {
          out[last] = out[last].slice(0, -5) + ' ' + inline(line.trim(), codes) + '</li>';
          continue;
        }
      }

      // blank
      if (!line.trim()) {
        flushPara();
        closeLists();
        continue;
      }

      para.push(line.trim());
    }
    flushPara();
    closeLists();
    flushQuote();

    return { html: unstash(out.join('\n'), codes), headings };
  }

  /* ------------------------------------------------------------------ state */
  const state = { docId: 'overview', anchor: null };
  const $ = (sel) => document.querySelector(sel);

  /* --------------------------------------------------------------- sidebar */
  function buildNav() {
    const groups = [];
    for (const d of DOCS) {
      let g = groups.find((x) => x.name === docGroup(d));
      if (!g) groups.push((g = { name: docGroup(d), items: [] }));
      g.items.push(d);
    }
    $('#nav').innerHTML = groups
      .map(
        (g) =>
          `<div class="nav-group"><div class="nav-group-title">${g.name}</div>` +
          g.items.map((d) => `<a class="nav-item" data-doc="${d.id}" href="#/${d.id}">${docTitle(d)}</a>`).join('') +
          `</div>`
      )
      .join('');
    document.querySelectorAll('.nav-item').forEach((a) => a.classList.toggle('active', a.dataset.doc === state.docId));
  }

  function applyChrome() {
    document.documentElement.lang = LANG;
    $('.toc-title').textContent = t('onThisPage');
    $('#search-input').placeholder = t('searchPlaceholder');
    $('#search-trigger span').textContent = t('searchTrigger');
    $('#lang-toggle').textContent = LANG === 'es' ? 'EN' : 'ES';
    $('#lang-toggle').title = LANG === 'es' ? 'Switch to English' : 'Cambiar a español';
  }

  /* ------------------------------------------------------------------ route */
  function parseHash() {
    const h = location.hash.replace(/^#\/?/, '');
    const [docId, ...rest] = h.split('/');
    return { docId: byId.has(docId) ? docId : 'overview', anchor: rest.join('/') || null };
  }

  let spy = null;

  function route() {
    const { docId, anchor } = parseHash();
    const changed = docId !== state.renderedId;
    state.docId = docId;
    state.anchor = anchor;

    if (changed) {
      const doc = byId.get(docId);
      const { html, headings } = render(docMd(doc));
      const article = $('#doc');
      const fallback = docIsFallback(doc)
        ? `<blockquote><p>${t('fallback')}</p></blockquote>`
        : '';
      article.innerHTML = `<div class="doc-kicker">${docGroup(doc)} · ${doc.file}</div>` + fallback + html;
      article.style.animation = 'none';
      void article.offsetWidth;
      article.style.animation = '';
      state.renderedId = docId;
      document.title = `${docTitle(doc)} · molde docs`;
      $('#doc-source').textContent = doc.file;

      // nav active
      document.querySelectorAll('.nav-item').forEach((a) => a.classList.toggle('active', a.dataset.doc === docId));

      // TOC
      $('#toc-list').innerHTML = headings
        .map((h) => `<a class="lvl-${h.lvl}" data-slug="${h.slug}" href="#/${docId}/${h.slug}">${esc(h.text)}</a>`)
        .join('');
      setupSpy();

      // pager
      const idx = DOCS.findIndex((d) => d.id === docId);
      const prev = DOCS[idx - 1];
      const next = DOCS[idx + 1];
      $('#pager').innerHTML =
        (prev ? `<a class="prev" href="#/${prev.id}"><span class="pager-label">${t('prev')}</span><span class="pager-title">${docTitle(prev)}</span></a>` : '<span></span>') +
        (next ? `<a class="next" href="#/${next.id}"><span class="pager-label">${t('next')}</span><span class="pager-title">${docTitle(next)}</span></a>` : '');

      closeSidebar();
    }

    if (anchor) {
      const el = document.getElementById(anchor);
      if (el) el.scrollIntoView({ block: 'start' });
    } else if (changed) {
      window.scrollTo(0, 0);
    }
  }

  function setupSpy() {
    if (spy) spy.disconnect();
    const links = [...document.querySelectorAll('#toc-list a')];
    if (!links.length) return;
    const bySlug = new Map(links.map((a) => [a.dataset.slug, a]));
    spy = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) {
            links.forEach((a) => a.classList.remove('active'));
            const a = bySlug.get(e.target.id);
            if (a) a.classList.add('active');
          }
        }
      },
      { rootMargin: '-10% 0px -75% 0px' }
    );
    document.querySelectorAll('.doc h2[id], .doc h3[id]').forEach((h) => spy.observe(h));
  }

  /* ----------------------------------------------------------------- search */
  let INDEX = [];
  function buildIndex() {
    INDEX = [];
    for (const d of DOCS) {
      const used = new Set();
      let current = { doc: d, heading: docTitle(d), slug: '', body: [] };
      const sections = [current];
      for (const line of docMd(d).split('\n')) {
        const h = line.match(/^#{1,3}\s+(.*)$/);
        if (h) {
          const text = h[1].replace(/[*_`#]/g, '').trim();
          current = { doc: d, heading: text, slug: slugify(text, used), body: [] };
          sections.push(current);
        } else {
          current.body.push(line);
        }
      }
      for (const s of sections) INDEX.push({ ...s, body: s.body.join(' ').replace(/[`>|#*]/g, ' ').replace(/\s+/g, ' ') });
    }
  }

  function search(query) {
    const q = query.trim().toLowerCase();
    if (q.length < 2) return [];
    const terms = q.split(/\s+/);
    const scored = [];
    for (const s of INDEX) {
      const heading = s.heading.toLowerCase();
      const body = s.body.toLowerCase();
      let score = 0;
      for (const term of terms) {
        if (heading.includes(term)) score += heading.startsWith(term) ? 30 : 18;
        if (docTitle(s.doc).toLowerCase().includes(term)) score += 10;
        const idx = body.indexOf(term);
        if (idx >= 0) score += 6;
        if (score === 0) { score = -1; break; }
      }
      if (score > 0) scored.push({ s, score });
    }
    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, 20).map(({ s }) => {
      const body = s.body;
      const idx = body.toLowerCase().indexOf(terms[0]);
      const from = Math.max(0, idx - 40);
      let snippet = (from > 0 ? '…' : '') + body.slice(from, from + 110) + '…';
      snippet = esc(snippet);
      for (const t of terms) {
        snippet = snippet.replace(new RegExp(`(${t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`, 'ig'), '<mark>$1</mark>');
      }
      return { ...s, snippet };
    });
  }

  let selIdx = 0;
  function renderResults(items) {
    const box = $('#search-results');
    $('#search-count').textContent = items.length ? t('results', items.length) : '';
    if (!items.length) {
      box.innerHTML = `<div class="search-empty">${t('empty')}</div>`;
      return;
    }
    selIdx = 0;
    box.innerHTML = items
      .map(
        (r, i) =>
          `<a class="search-result${i === 0 ? ' selected' : ''}" href="#/${r.doc.id}${r.slug ? '/' + r.slug : ''}">` +
          `<span class="r-doc">${docGroup(r.doc)} / ${docTitle(r.doc)}</span>` +
          `<div class="r-heading">${esc(r.heading)}</div>` +
          `<div class="r-snippet">${r.snippet}</div></a>`
      )
      .join('');
  }

  function openSearch() {
    $('#search-overlay').hidden = false;
    $('#search-input').value = '';
    renderResults([]);
    $('#search-results').innerHTML = '';
    $('#search-count').textContent = '';
    $('#search-input').focus();
  }
  function closeSearch() {
    $('#search-overlay').hidden = true;
  }

  /* ------------------------------------------------------------------ theme */
  function applyTheme(t) {
    document.documentElement.dataset.theme = t;
    try { localStorage.setItem('molde-docs-theme', t); } catch (e) { /* private mode */ }
  }

  /* ----------------------------------------------------------------- mobile */
  function closeSidebar() {
    $('#sidebar').classList.remove('open');
    $('#backdrop').classList.remove('show');
  }

  function setLang(lang) {
    LANG = lang;
    try { localStorage.setItem('molde-docs-lang', lang); } catch (e) { /* ignore */ }
    buildNav();
    buildIndex();
    applyChrome();
    state.renderedId = null; // force re-render of the current doc
    route();
  }

  /* ------------------------------------------------------------------- wire */
  buildNav();
  buildIndex();
  applyChrome();

  try {
    const saved = localStorage.getItem('molde-docs-theme');
    if (saved) applyTheme(saved);
    else if (window.matchMedia('(prefers-color-scheme: light)').matches) applyTheme('light');
  } catch (e) { /* ignore */ }

  window.addEventListener('hashchange', route);
  route();

  $('#theme-toggle').addEventListener('click', () => {
    applyTheme(document.documentElement.dataset.theme === 'dark' ? 'light' : 'dark');
  });

  $('#lang-toggle').addEventListener('click', () => setLang(LANG === 'es' ? 'en' : 'es'));

  $('#search-trigger').addEventListener('click', openSearch);
  $('#search-overlay').addEventListener('click', (e) => {
    if (e.target === e.currentTarget) closeSearch();
  });
  $('#search-input').addEventListener('input', (e) => renderResults(search(e.target.value)));
  $('#search-results').addEventListener('click', () => closeSearch());

  $('#hamburger').addEventListener('click', () => {
    $('#sidebar').classList.toggle('open');
    $('#backdrop').classList.toggle('show');
  });
  $('#backdrop').addEventListener('click', closeSidebar);

  document.addEventListener('keydown', (e) => {
    const searchOpen = !$('#search-overlay').hidden;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      searchOpen ? closeSearch() : openSearch();
      return;
    }
    if (searchOpen) {
      const results = [...document.querySelectorAll('.search-result')];
      if (e.key === 'Escape') closeSearch();
      else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        if (!results.length) return;
        results[selIdx]?.classList.remove('selected');
        selIdx = (selIdx + (e.key === 'ArrowDown' ? 1 : results.length - 1)) % results.length;
        results[selIdx].classList.add('selected');
        results[selIdx].scrollIntoView({ block: 'nearest' });
      } else if (e.key === 'Enter' && results[selIdx]) {
        location.hash = new URL(results[selIdx].href).hash;
        closeSearch();
      }
      return;
    }
    if (e.key === '/' && !/^(input|textarea)$/i.test(document.activeElement.tagName)) {
      e.preventDefault();
      openSearch();
    }
  });

  // copy buttons (delegated)
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('.code-copy');
    if (!btn) return;
    const code = btn.parentElement.querySelector('code');
    navigator.clipboard.writeText(code.textContent).then(() => {
      btn.classList.add('done');
      setTimeout(() => btn.classList.remove('done'), 1200);
    });
  });
})();
