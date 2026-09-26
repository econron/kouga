import http from 'k6/http';
import { check } from 'k6';

// Run one mode at a time against the same generated Taskboard image and DB.
const base = __ENV.BASE_URL || 'http://127.0.0.1:18086';
const mode = __ENV.MODE || 'json';
const auth = { Authorization: `Bearer ${__ENV.TOKEN || ''}` };
const json = { ...auth, 'Content-Type': 'application/json' };

export default function () {
  if (mode === 'json') {
    check(http.get(`${base}/health`), { 'health 200': (r) => r.status === 200 });
    return;
  }
  if (mode === 'read') {
    check(http.get(`${base}/tasks/${__ENV.TASK_ID}`, { headers: auth }), {
      'owned task 200': (r) => r.status === 200,
    });
    return;
  }
  if (mode !== 'crud') throw new Error(`unknown MODE: ${mode}`);

  const created = http.post(
    `${base}/tasks`,
    JSON.stringify({ project_id: __ENV.PROJECT_ID, title: `bench-${__VU}-${__ITER}` }),
    { headers: json },
  );
  const id = created.json('data.id');
  if (!check(created, { 'create 201': (r) => r.status === 201 && !!id })) return;
  check(http.get(`${base}/tasks/${id}`, { headers: auth }), {
    'show 200': (r) => r.status === 200,
  });
  check(http.patch(`${base}/tasks/${id}`, JSON.stringify({ completed: true }), { headers: json }), {
    'complete 200': (r) => r.status === 200,
  });
  check(http.del(`${base}/tasks/${id}`, null, { headers: auth }), {
    'delete 204': (r) => r.status === 204,
  });
}
