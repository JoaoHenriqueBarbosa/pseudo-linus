// ---- APIs da janela sem tela nem hardware que o Chromium headless expõe: `viewport` (Viewport), `crashReport`
// (CrashReportContext), `launchQueue` (LaunchQueue), `speechSynthesis` (SpeechSynthesis, sem vozes e sem ativação
// do usuário: toda fala termina em `not-allowed`) e `scheduler` (Scheduler, TaskController, TaskSignal). Formas,
// retornos, mensagens e a ordem das tarefas medidos no oráculo.

const enumValue = (allowed, value) => allowed.includes(String(value));
const TASK_PRIORITIES = ['user-blocking', 'user-visible', 'background'];
const priorityError = (prefix, value) => new TypeError(`${prefix}The provided value '${String(value)}' is not a valid enum value of type TaskPriority.`);

class Viewport {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('Viewport');
    hidden(this, '__viewport', true);
  }
  // Sem dobra de tela: um segmento só, a área visível inteira, numa lista nova a cada leitura.
  get segments() { receiver(this, P.__viewport); return [new DOMRect(0, 0, innerWidth, innerHeight)]; }
}
webInterface(Viewport, 'Viewport');

class CrashReportContext {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('CrashReportContext');
    hidden(this, '__crashReport', { initialized: false, ready: false, entries: new Map() });
  }
  delete(key) {
    receiver(this, P.__crashReport);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'delete' on 'CrashReportContext': 1 argument required, but only 0 present.");
    const state = this[P.__crashReport];
    if (!state.ready) throw domException("Failed to execute 'delete' on 'CrashReportContext': CrashReportContext is not initialized. Call initialize() and wait for it to resolve.", 'InvalidStateError');
    state.entries.delete(String(key));
  }
  initialize(length) {
    receiver(this, P.__crashReport);
    if (arguments.length < 1) return Promise.reject(new TypeError("Failed to execute 'initialize' on 'CrashReportContext': 1 argument required, but only 0 present."));
    const state = this[P.__crashReport];
    if (state.initialized) return Promise.reject(domException("Failed to execute 'initialize' on 'CrashReportContext': The initialize() method has already been called.", 'InvalidStateError'));
    state.initialized = true;
    return new Promise((resolve) => setTimeout(() => { state.ready = true; resolve(); }, 0));
  }
  set(key, value) {
    receiver(this, P.__crashReport);
    if (arguments.length < 2) throw new TypeError(`Failed to execute 'set' on 'CrashReportContext': 2 arguments required, but only ${arguments.length} present.`);
    const state = this[P.__crashReport];
    if (!state.ready) throw domException("Failed to execute 'set' on 'CrashReportContext': CrashReportContext is not initialized. Call initialize() and wait for it to resolve.", 'InvalidStateError');
    state.entries.set(String(key), String(value));
  }
}
webInterface(CrashReportContext, 'CrashReportContext');

class LaunchParams {
  constructor() { throw illegalConstructor('LaunchParams'); }
}
webInterface(LaunchParams, 'LaunchParams');
class LaunchQueue {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('LaunchQueue');
    hidden(this, '__launchQueue', true);
  }
  // A página não foi aberta como aplicativo: não há parâmetros de abertura a entregar ao consumidor.
  setConsumer(consumer) {
    receiver(this, P.__launchQueue);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'setConsumer' on 'LaunchQueue': 1 argument required, but only 0 present.");
    if (typeof consumer !== 'function') throw new TypeError("Failed to execute 'setConsumer' on 'LaunchQueue': parameter 1 is not of type 'Function'.");
  }
}
webInterface(LaunchQueue, 'LaunchQueue');

// ---- Síntese de fala.
const finiteFloat = (name, value) => {
  const number = Number(value);
  if (!Number.isFinite(number)) throw new TypeError(`Failed to set the '${name}' property on 'SpeechSynthesisUtterance': The provided float value is non-finite.`);
  return Math.fround(number);
};
class SpeechSynthesisVoice {
  constructor() { throw illegalConstructor('SpeechSynthesisVoice'); }
}
webInterface(SpeechSynthesisVoice, 'SpeechSynthesisVoice');
class SpeechSynthesisUtterance extends EventTarget {
  constructor(text) {
    super();
    hidden(this, '__utterance', { text: text === undefined ? '' : String(text), lang: '', voice: null, volume: 1, rate: 1, pitch: 1 });
  }
  get text() { receiver(this, P.__utterance); return this[P.__utterance].text; }
  set text(value) { receiver(this, P.__utterance); this[P.__utterance].text = String(value); }
  get lang() { receiver(this, P.__utterance); return this[P.__utterance].lang; }
  set lang(value) { receiver(this, P.__utterance); this[P.__utterance].lang = String(value); }
  get voice() { receiver(this, P.__utterance); return this[P.__utterance].voice; }
  set voice(value) {
    receiver(this, P.__utterance);
    if (value !== null && !(value instanceof SpeechSynthesisVoice)) throw new TypeError("Failed to set the 'voice' property on 'SpeechSynthesisUtterance': Failed to convert value to 'SpeechSynthesisVoice'.");
    this[P.__utterance].voice = value;
  }
  get volume() { receiver(this, P.__utterance); return this[P.__utterance].volume; }
  set volume(value) { receiver(this, P.__utterance); this[P.__utterance].volume = Math.min(1, Math.max(0, finiteFloat('volume', value))); }
  get rate() { receiver(this, P.__utterance); return this[P.__utterance].rate; }
  set rate(value) { receiver(this, P.__utterance); this[P.__utterance].rate = Math.min(10, Math.max(0.1, finiteFloat('rate', value))); }
  get pitch() { receiver(this, P.__utterance); return this[P.__utterance].pitch; }
  set pitch(value) { receiver(this, P.__utterance); this[P.__utterance].pitch = Math.min(2, Math.max(0, finiteFloat('pitch', value))); }
}
defineHandlerProperties(SpeechSynthesisUtterance.prototype, ['start', 'end', 'error', 'pause', 'resume', 'mark', 'boundary']);
webInterface(SpeechSynthesisUtterance, 'SpeechSynthesisUtterance');

const requiredMember = (iface, dictionary, member) => new TypeError(`Failed to construct '${iface}': Failed to read the '${member}' property from '${dictionary}': Required member is undefined.`);
class SpeechSynthesisEvent extends Event {
  constructor(type, init) {
    if (arguments.length < 2) throw new TypeError(`Failed to construct '${new.target.name}': 2 arguments required, but only ${arguments.length} present.`);
    if (init === null || init === undefined || init.utterance === undefined) throw requiredMember(new.target.name, new.target.name + 'Init', 'utterance');
    if (!(init.utterance instanceof SpeechSynthesisUtterance)) throw new TypeError(`Failed to construct '${new.target.name}': Failed to read the 'utterance' property from '${new.target.name}Init': Failed to convert value to 'SpeechSynthesisUtterance'.`);
    super(type, init);
    hidden(this, '__speech', { utterance: init.utterance, charIndex: (init.charIndex >>> 0) || 0, charLength: (init.charLength >>> 0) || 0, elapsedTime: Math.fround(Number(init.elapsedTime) || 0), name: init.name === undefined ? '' : String(init.name) });
  }
  get utterance() { return this[P.__speech].utterance; }
  get charIndex() { return this[P.__speech].charIndex; }
  get charLength() { return this[P.__speech].charLength; }
  get elapsedTime() { return this[P.__speech].elapsedTime; }
  get name() { return this[P.__speech].name; }
}
webInterface(SpeechSynthesisEvent, 'SpeechSynthesisEvent');
const SPEECH_ERRORS = ['canceled', 'interrupted', 'audio-busy', 'audio-hardware', 'network', 'synthesis-unavailable', 'synthesis-failed', 'language-unavailable', 'voice-unavailable', 'text-too-long', 'invalid-argument', 'not-allowed'];
class SpeechSynthesisErrorEvent extends SpeechSynthesisEvent {
  constructor(type, init) {
    if (arguments.length >= 2 && init !== null && init !== undefined && init.utterance !== undefined && init.error === undefined) throw requiredMember('SpeechSynthesisErrorEvent', 'SpeechSynthesisErrorEventInit', 'error');
    super(...arguments);
    if (!enumValue(SPEECH_ERRORS, init.error)) throw new TypeError(`Failed to construct 'SpeechSynthesisErrorEvent': Failed to read the 'error' property from 'SpeechSynthesisErrorEventInit': The provided value '${String(init.error)}' is not a valid enum value of type SpeechSynthesisErrorCode.`);
    hidden(this, '__speechError', String(init.error));
  }
  get error() { return this[P.__speechError]; }
}
webInterface(SpeechSynthesisErrorEvent, 'SpeechSynthesisErrorEvent');

class SpeechSynthesis extends EventTarget {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('SpeechSynthesis');
    super();
    hidden(this, '__speechSynthesis', true);
  }
  get pending() { receiver(this, P.__speechSynthesis); return false; }
  get speaking() { receiver(this, P.__speechSynthesis); return false; }
  get paused() { receiver(this, P.__speechSynthesis); return false; }
  cancel() { receiver(this, P.__speechSynthesis); }
  getVoices() { receiver(this, P.__speechSynthesis); return []; }
  pause() { receiver(this, P.__speechSynthesis); }
  resume() { receiver(this, P.__speechSynthesis); }
  // Sem ativação do usuário o Chromium recusa a fala na hora: o `error` é despachado dentro do próprio `speak`.
  speak(utterance) {
    receiver(this, P.__speechSynthesis);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'speak' on 'SpeechSynthesis': 1 argument required, but only 0 present.");
    if (!(utterance instanceof SpeechSynthesisUtterance)) throw new TypeError("Failed to execute 'speak' on 'SpeechSynthesis': parameter 1 is not of type 'SpeechSynthesisUtterance'.");
    utterance.dispatchEvent(new SpeechSynthesisErrorEvent('error', { utterance, error: 'not-allowed', elapsedTime: performance.now() }));
  }
}
defineHandlerProperties(SpeechSynthesis.prototype, ['voiceschanged']);
webInterface(SpeechSynthesis, 'SpeechSynthesis');
const speechSynthesis = new SpeechSynthesis(ILLEGAL);
// A lista de vozes (vazia) termina de carregar logo depois da página: um `voiceschanged`.
setTimeout(() => speechSynthesis.dispatchEvent(new Event('voiceschanged')), 0);

// ---- Agendamento priorizado de tarefas.
// Os sinais de tarefa herdam do `AbortSignal` do motor, cujo `dispatchEvent` só aceita o `Event` do motor (o global
// ainda é ele aqui; a passagem final do prelúdio o troca pelo do navegador): o evento de prioridade o estende.
const EngineEvent = globalThis.Event;
class TaskPriorityChangeEvent extends EngineEvent {
  constructor(type, init) {
    if (arguments.length < 2) throw new TypeError(`Failed to construct 'TaskPriorityChangeEvent': 2 arguments required, but only ${arguments.length} present.`);
    if (init === null || init === undefined || init.previousPriority === undefined) throw requiredMember('TaskPriorityChangeEvent', 'TaskPriorityChangeEventInit', 'previousPriority');
    if (!enumValue(TASK_PRIORITIES, init.previousPriority)) throw priorityError("Failed to construct 'TaskPriorityChangeEvent': Failed to read the 'previousPriority' property from 'TaskPriorityChangeEventInit': ", init.previousPriority);
    super(type, init);
    hidden(this, '__previousPriority', String(init.previousPriority));
  }
  get previousPriority() { return this[P.__previousPriority]; }
}
webInterface(TaskPriorityChangeEvent, 'TaskPriorityChangeEvent');

class TaskSignal extends AbortSignal {
  constructor() { throw illegalConstructor('TaskSignal'); }
  get priority() { receiver(this, P.__taskSignal); return this[P.__taskSignal].priority; }
  // `AbortSignal.any` com prioridade: a do `TaskSignal` de origem (que acompanha as mudanças dele) ou a fixa.
  static any(signals, init) {
    const signal = AbortSignal.any(signals);
    const source = init && init.priority !== undefined ? init.priority : 'user-visible';
    if (!(source instanceof TaskSignal) && !enumValue(TASK_PRIORITIES, source)) throw priorityError("Failed to execute 'any' on 'TaskSignal': Failed to read the 'priority' property from 'TaskSignalAnyInit': ", source);
    Object.setPrototypeOf(signal, TaskSignal.prototype);
    const state = { priority: source instanceof TaskSignal ? source.priority : String(source), changing: false };
    hidden(signal, '__taskSignal', state);
    if (source instanceof TaskSignal) source.addEventListener('prioritychange', () => changePriority(signal, source.priority));
    return signal;
  }
}
defineHandlerProperties(TaskSignal.prototype, ['prioritychange']);
webInterface(TaskSignal, 'TaskSignal');
const changePriority = (signal, priority) => {
  const state = signal[P.__taskSignal];
  if (state.changing) throw domException("Failed to execute 'setPriority' on 'TaskController': Cannot change priority when a prioritychange event is in progress.", 'NotAllowedError');
  if (state.priority === priority) return;
  const previousPriority = state.priority;
  state.priority = priority;
  state.changing = true;
  try { signal.dispatchEvent(new TaskPriorityChangeEvent('prioritychange', { previousPriority })); } finally { state.changing = false; }
};

class TaskController extends AbortController {
  constructor(init) {
    const priority = init && init.priority !== undefined ? init.priority : 'user-visible';
    if (!enumValue(TASK_PRIORITIES, priority)) throw priorityError("Failed to construct 'TaskController': Failed to read the 'priority' property from 'TaskControllerInit': ", priority);
    super();
    Object.setPrototypeOf(this.signal, TaskSignal.prototype);
    hidden(this.signal, '__taskSignal', { priority: String(priority), changing: false });
    hidden(this, '__taskController', true);
  }
  setPriority(priority) {
    receiver(this, P.__taskController);
    if (arguments.length < 1) throw new TypeError("Failed to execute 'setPriority' on 'TaskController': 1 argument required, but only 0 present.");
    if (!enumValue(TASK_PRIORITIES, priority)) throw priorityError("Failed to execute 'setPriority' on 'TaskController': ", priority);
    changePriority(this.signal, String(priority));
  }
}
webInterface(TaskController, 'TaskController');

// As filas do agendador do Blink, da mais urgente para a menos: cada prioridade com a fila das continuações do
// `yield` à frente. Cada tarefa postada reserva uma vez na fila de tarefas do laço (um timer de zero), na ordem
// em que foi postada; quando a vez chega, roda a tarefa pendente mais urgente, não necessariamente a dela.
const SCHEDULER_QUEUES = ['user-blocking:continuation', 'user-blocking', 'user-visible:continuation', 'user-visible', 'background:continuation', 'background'];
const schedulerQueues = new Map(SCHEDULER_QUEUES.map((name) => [name, []]));
let runningTaskPriority = null;
const effectivePriority = (task) => (task.priority !== undefined ? task.priority : task.signal instanceof TaskSignal ? task.signal.priority : 'user-visible');
const runSchedulerTurn = () => {
  for (const name of SCHEDULER_QUEUES) {
    const queue = schedulerQueues.get(name);
    for (let index = 0; index < queue.length; index++) {
      const task = queue[index];
      // Uma tarefa cujo sinal mudou de prioridade passa para a fila nova.
      const wanted = effectivePriority(task) + (task.continuation ? ':continuation' : '');
      if (wanted !== name) { queue.splice(index--, 1); schedulerQueues.get(wanted).push(task); continue; }
      queue.splice(index, 1);
      task.run();
      return;
    }
  }
};
const enqueueSchedulerTask = (task) => {
  schedulerQueues.get(effectivePriority(task) + (task.continuation ? ':continuation' : '')).push(task);
  setTimeout(runSchedulerTurn, 0);
};
const abortReason = (signal) => signal.reason;
class Scheduler {
  constructor(key) {
    if (key !== ILLEGAL) throw illegalConstructor('Scheduler');
    hidden(this, '__scheduler', true);
  }
  postTask(callback, options) {
    receiver(this, P.__scheduler);
    if (arguments.length < 1) return Promise.reject(new TypeError("Failed to execute 'postTask' on 'Scheduler': 1 argument required, but only 0 present."));
    if (typeof callback !== 'function') return Promise.reject(new TypeError("Failed to execute 'postTask' on 'Scheduler': parameter 1 is not of type 'Function'."));
    const init = options === undefined || options === null ? {} : options;
    if (init.priority !== undefined && !enumValue(TASK_PRIORITIES, init.priority)) {
      return Promise.reject(priorityError("Failed to execute 'postTask' on 'Scheduler': Failed to read the 'priority' property from 'SchedulerPostTaskOptions': ", init.priority));
    }
    const signal = init.signal;
    if (signal !== undefined && signal !== null && signal.aborted) return Promise.reject(abortReason(signal));
    const delay = Math.max(0, Number(init.delay) || 0);
    return new Promise((resolve, reject) => {
      const task = {
        priority: init.priority === undefined ? undefined : String(init.priority),
        signal,
        continuation: false,
        done: false,
        run() {
          if (task.done) return;
          task.done = true;
          const previous = runningTaskPriority;
          runningTaskPriority = effectivePriority(task);
          try { resolve(callback()); } catch (error) { reject(error); } finally { runningTaskPriority = previous; }
        },
      };
      if (signal) {
        signal.addEventListener('abort', () => {
          if (task.done) return;
          task.done = true;
          reject(abortReason(signal));
        }, { once: true });
      }
      if (delay > 0) setTimeout(() => enqueueSchedulerTask(task), delay);
      else enqueueSchedulerTask(task);
    });
  }
  // A continuação herda a prioridade da tarefa que cedeu (fora de tarefa do agendador, `user-visible`).
  yield() {
    receiver(this, P.__scheduler);
    return new Promise((resolve) => {
      enqueueSchedulerTask({ priority: runningTaskPriority || 'user-visible', continuation: true, done: false, run() { resolve(); } });
    });
  }
}
webInterface(Scheduler, 'Scheduler');
const scheduler = new Scheduler(ILLEGAL);

const viewport = new Viewport(ILLEGAL);
const crashReport = new CrashReportContext(ILLEGAL);
const launchQueue = new LaunchQueue(ILLEGAL);
replaceable('viewport', () => viewport);
readonly('crashReport', () => crashReport);
readonly('launchQueue', () => launchQueue);
readonly('speechSynthesis', () => speechSynthesis);
replaceable('scheduler', () => scheduler);
const windowApis = { Viewport, CrashReportContext, LaunchParams, LaunchQueue, SpeechSynthesis, SpeechSynthesisEvent, SpeechSynthesisErrorEvent, SpeechSynthesisVoice, SpeechSynthesisUtterance, Scheduler, TaskController, TaskSignal, TaskPriorityChangeEvent };
Object.assign(globalThis, windowApis);
for (const key of Object.keys(windowApis)) Object.defineProperty(globalThis, key, { enumerable: false });
