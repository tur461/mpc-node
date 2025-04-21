/**
 * @packageDocumentation
 *
 * Race an event against an AbortSignal, taking care to remove any event
 * listeners that were added.
 *
 * @example Getting started
 *
 * ```TypeScript
 * import { raceEvent } from 'race-event'
 *
 * const controller = new AbortController()
 * const emitter = new EventTarget()
 *
 * setTimeout(() => {
 *   controller.abort()
 * }, 500)
 *
 * setTimeout(() => {
 *   // too late
 *   emitter.dispatchEvent(new CustomEvent('event'))
 * }, 1000)
 *
 * // throws an AbortError
 * const resolve = await raceEvent(emitter, 'event', controller.signal)
 * ```
 *
 * @example Aborting the promise with an error event
 *
 * ```TypeScript
 * import { raceEvent } from 'race-event'
 *
 * const emitter = new EventTarget()
 *
 * setTimeout(() => {
 *   emitter.dispatchEvent(new CustomEvent('failure', {
 *     detail: new Error('Oh no!')
 *   }))
 * }, 1000)
 *
 * // throws 'Oh no!' error
 * const resolve = await raceEvent(emitter, 'success', AbortSignal.timeout(5000), {
 *   errorEvent: 'failure'
 * })
 * ```
 *
 * @example Customising the thrown AbortError
 *
 * The error message and `.code` property of the thrown `AbortError` can be
 * specified by passing options:
 *
 * ```TypeScript
 * import { raceEvent } from 'race-event'
 *
 * const controller = new AbortController()
 * const emitter = new EventTarget()
 *
 * setTimeout(() => {
 *   controller.abort()
 * }, 500)
 *
 * // throws a Error: Oh no!
 * const resolve = await raceEvent(emitter, 'event', controller.signal, {
 *   errorMessage: 'Oh no!',
 *   errorCode: 'ERR_OH_NO'
 * })
 * ```
 *
 * @example Only resolving on specific events
 *
 * Where multiple events with the same type are emitted, a `filter` function can
 * be passed to only resolve on one of them:
 *
 * ```TypeScript
 * import { raceEvent } from 'race-event'
 *
 * const controller = new AbortController()
 * const emitter = new EventTarget()
 *
 * // throws a Error: Oh no!
 * const resolve = await raceEvent(emitter, 'event', controller.signal, {
 *   filter: (evt: Event) => {
 *     return evt.detail.foo === 'bar'
 *   }
 * })
 * ```
 *
 * @example Terminating early by throwing from the filter
 *
 * You can cause listening for the event to cease and all event listeners to be
 * removed by throwing from the filter:
 *
 * ```TypeScript
 * import { raceEvent } from 'race-event'
 *
 * const controller = new AbortController()
 * const emitter = new EventTarget()
 *
 * // throws Error: Cannot continue
 * const resolve = await raceEvent(emitter, 'event', controller.signal, {
 *   filter: (evt) => {
 *     if (...reasons) {
 *       throw new Error('Cannot continue')
 *     }
 *
 *     return true
 *   }
 * })
 * ```
 */

/**
 * An abort error class that extends error
 */
export class AbortError extends Error {
  public type: string
  public code: string | string

  constructor (message?: string, code?: string) {
    super(message ?? 'The operation was aborted')
    this.type = 'aborted'
    this.name = 'AbortError'
    this.code = code ?? 'ABORT_ERR'
  }
}

export interface RaceEventOptions<T> {
  /**
   * The message for the error thrown if the signal aborts
   */
  errorMessage?: string

  /**
   * The code for the error thrown if the signal aborts
   */
  errorCode?: string

  /**
   * The name of an event emitted on the emitter that should cause the returned
   * promise to reject. The rejection reason will be the `.detail` field of the
   * event.
   */
  errorEvent?: string

  /**
   * When multiple events with the same name may be emitted, pass a filter
   * function here to allow ignoring ones that should not cause the returned
   * promise to resolve.
   */
  filter?(evt: T): boolean
}

/**
 * Race a promise against an abort signal
 */
export async function raceEvent <T> (emitter: EventTarget, eventName: string, signal?: AbortSignal, opts?: RaceEventOptions<T>): Promise<T> {
  // create the error here so we have more context in the stack trace
  const error = new AbortError(opts?.errorMessage, opts?.errorCode)

  if (signal?.aborted === true) {
    return Promise.reject(error)
  }

  return new Promise((resolve, reject) => {
    function removeListeners (): void {
      signal?.removeEventListener('abort', abortListener)
      emitter.removeEventListener(eventName, eventListener)

      if (opts?.errorEvent != null) {
        emitter.removeEventListener(opts.errorEvent, errorEventListener)
      }
    }

    const eventListener = (evt: any): void => {
      try {
        if (opts?.filter?.(evt) === false) {
          return
        }
      } catch (err: any) {
        removeListeners()
        reject(err)
        return
      }

      removeListeners()
      resolve(evt)
    }

    const errorEventListener = (evt: any): void => {
      removeListeners()
      reject(evt.detail)
    }

    const abortListener = (): void => {
      removeListeners()
      reject(error)
    }

    signal?.addEventListener('abort', abortListener)
    emitter.addEventListener(eventName, eventListener)

    if (opts?.errorEvent != null) {
      emitter.addEventListener(opts.errorEvent, errorEventListener)
    }
  })
}
