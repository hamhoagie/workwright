const API = '/api';

async function loadFeed() {
    const el = document.getElementById('feed');
    try {
        const resp = await fetch(`${API}/tasks`);
        const tasks = await resp.json();

        if (!tasks.length) {
            el.innerHTML = `<div class="feed-empty">No tasks yet. <a href="/brief">Brief a wright</a> to get started.</div>`;
            return;
        }

        el.innerHTML = tasks.map((t, i) => {
            const score = t.taste_score;
            const scoreClass = score > 0 ? 'positive' : score < 0 ? 'negative' : '';
            const submitter = t.submitted_by_name || t.agent_id || '';
            const time = relativeTime(t.created);

            return `
            <div class="feed-item">
                <div class="feed-header" onclick="toggle(${i})">
                    <div class="feed-status ${t.status}"></div>
                    <div class="feed-intent">${esc(t.intent)}</div>
                    <div class="feed-scope">${esc(t.scope)}</div>
                </div>
                <div class="feed-details" id="details-${i}">
                    <div class="feed-why">${esc(t.why)}</div>
                    <div class="feed-meta">${submitter ? esc(submitter) + ' · ' : ''}${time} · ${t.status}</div>
                    ${t.defense ? `<div class="feed-defense">${esc(t.defense)}</div>` : ''}
                    ${score !== null ? `<div class="feed-score ${scoreClass}">Score: ${score > 0 ? '+' : ''}${score}${t.taste_note ? ' — ' + esc(t.taste_note) : ''}</div>` : ''}
                </div>
            </div>`;
        }).join('');
    } catch (e) {
        el.innerHTML = `<div class="feed-empty">Could not load feed.</div>`;
    }
}

async function loadTaste() {
    const el = document.getElementById('taste-guide');
    try {
        const resp = await fetch(`${API}/taste`);
        const data = await resp.json();
        el.textContent = data.text || 'No taste signals yet.';
    } catch (e) {
        el.textContent = 'Could not load taste guide.';
    }
}

function toggle(i) {
    document.getElementById(`details-${i}`).classList.toggle('open');
}

function relativeTime(ts) {
    const diff = Date.now() / 1000 - ts;
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    if (diff < 604800) return `${Math.floor(diff / 86400)}d ago`;
    return new Date(ts * 1000).toLocaleDateString();
}

function esc(s) {
    if (!s) return '';
    const d = document.createElement('div');
    d.textContent = s;
    return d.innerHTML;
}

loadFeed();
loadTaste();