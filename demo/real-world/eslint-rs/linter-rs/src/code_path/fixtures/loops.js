for (let i = 0; i < n; i++) { if (i) continue; if (j) break; }
for (;;) { if (a) break; }
while (a) { if (b) break; c(); }
do { if (x) continue; } while (y);
outer: for (const k of o) { for (const m in k) { if (m) continue outer; if (n) break outer; } }
while (true) {}
foo();
