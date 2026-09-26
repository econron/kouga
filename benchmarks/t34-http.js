import http from 'k6/http';
import { check } from 'k6';

// Run each mode separately with a fixed VU count and duration. See docs/release-verification.md.
const base = __ENV.BASE_URL || 'http://127.0.0.1:18084';
const mode = __ENV.MODE || 'json';
const taskId = __ENV.TASK_ID;
const headers = { 'Content-Type': 'application/json' };

export default function () {
  if (mode === 'json') {
    check(http.get(`${base}/health`, { tags: { name: 'json' } }), {
      'JSON 200': (r) => r.status === 200,
    });
    return;
  }
  if (mode === 'read') {
    check(http.get(`${base}/tasks/${taskId}`, { tags: { name: 'db_read' } }), {
      'DB read 200': (r) => r.status === 200,
    });
    return;
  }
  if (mode !== 'crud') throw new Error(`unknown MODE: ${mode}`);

  const title = `bench-${__VU}-${__ITER}`;
  const created = http.post(`${base}/tasks`, JSON.stringify({ title, completed: false }), {
    headers,
    tags: { name: 'crud_create' },
  });
  const id = created.json('data.id');
  if (!check(created, { 'create 201': (r) => r.status === 201 && !!id })) return;
  check(http.get(`${base}/tasks/${id}`, { tags: { name: 'crud_show' } }), {
    'show 200': (r) => r.status === 200,
  });
  check(http.patch(`${base}/tasks/${id}`, JSON.stringify({ completed: true }), {
    headers,
    tags: { name: 'crud_update' },
  }), { 'update 200': (r) => r.status === 200 });
  check(http.del(`${base}/tasks/${id}`, null, { tags: { name: 'crud_delete' } }), {
    'delete 204': (r) => r.status === 204,
  });
}
