const d = document.createElement('div'); let r = []; try { d.innerHTML = 'a'; r.push('first ok'); } catch (e) { r.push('first ' + e.name); } window.R1 = r;
