try { await import('./bad.ts'); } catch (e: any) { console.log(e.name, e.message, e.position?.line, e.position?.column, e.position?.lineText); }
