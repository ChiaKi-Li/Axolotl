import { afterEach, expect, it } from 'vitest'
import { defineComponent, effectScope, h, nextTick, ref } from 'vue'

import Combobox from '../components/base/Combobox.vue'
import MultiSelect from '../components/base/MultiSelect.vue'
import TeleportOverflowMenu from '../components/base/TeleportOverflowMenu.vue'
import NewModal from '../components/modal/NewModal.vue'
import { useBodyScrollLock } from '../composables/body-scroll-lock'
import { I18N_INJECTION_KEY } from '../providers/i18n'
import { mountThemed, waitFor } from './visual-harness'

const cleanup: (() => void)[] = []
afterEach(() =>
    cleanup
        .splice(0)
        .reverse()
        .forEach((fn) => fn()),
)

async function modal(child: ReturnType<typeof h>) {
    const teleports = document.createElement('div')
    teleports.id = 'teleports'
    document.body.append(teleports)
    cleanup.push(() => teleports.remove())
    const wrapper = await mountThemed(NewModal, { header: 'Export logs' }, 'dark', {
        slots: { default: () => child },
        global: {
            provide: {
                [I18N_INJECTION_KEY as symbol]: {
                    locale: ref('en-US'),
                    t: (key: string) => key,
                    setLocale: () => undefined,
                },
            },
        },
    })
    cleanup.push(() => wrapper.unmount())
    const vm = wrapper.vm as unknown as { show: () => void; hide: () => Promise<void> }
    vm.show()
    await waitFor(() => !!document.querySelector('.modal-container.shown'))
    await waitFor(() => getComputedStyle(document.querySelector('.modal-body')!).opacity === '1')
    return { wrapper, vm }
}

function escape(target: Element) {
    target.dispatchEvent(
        new KeyboardEvent('keydown', {
            key: 'Escape',
            bubbles: true,
            cancelable: true,
        }),
    )
}

for (const kind of ['combobox', 'searchable-combobox', 'multiselect'] as const) {
    it(`closes only the ${kind} on the first Escape and the modal on the next`, async () => {
        const options = [
            { value: 'all', label: 'All logs' },
            { value: 'today', label: 'Today' },
        ]
        const child =
            kind === 'multiselect'
                ? h(MultiSelect, { options, modelValue: [] })
                : h(Combobox, {
                      options,
                      modelValue: 'all',
                      searchable: kind === 'searchable-combobox',
                  })
        await modal(child)
        const trigger = document.querySelector<HTMLElement>(
            kind === 'searchable-combobox' ? '[data-combobox] input' : '[aria-haspopup="listbox"]',
        )!
        trigger.focus()
        trigger.dispatchEvent(
            new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }),
        )
        await waitFor(() => !!document.querySelector('[role="listbox"]'))
        escape(
            kind === 'searchable-combobox' ? trigger : document.querySelector('[role="listbox"]')!,
        )
        await waitFor(() => !document.querySelector('[role="listbox"]'))
        expect(document.querySelector('.modal-container.shown')).not.toBeNull()
        await waitFor(() => document.activeElement === trigger)
        escape(trigger)
        await waitFor(() => !document.querySelector('[role="dialog"]'))
    })
}

it('respects Escape already consumed by a child control', async () => {
    await modal(
        h('button', { onKeydown: (event: KeyboardEvent) => event.preventDefault() }, 'Child'),
    )
    escape(document.querySelector('.modal-body button:last-child')!)
    expect(document.querySelector('.modal-container.shown')).not.toBeNull()
})

it('consumes Escape from multiselect search actions and restores its trigger', async () => {
    await modal(
        h(
            MultiSelect,
            {
                options: [{ value: 'all', label: 'All logs' }],
                modelValue: [],
                searchable: true,
            },
            { 'search-actions': () => h('button', { id: 'search-action' }, 'Action') },
        ),
    )
    const trigger = document.querySelector<HTMLElement>('[aria-haspopup="listbox"]')!
    trigger.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }),
    )
    await waitFor(() => !!document.querySelector('#search-action'))
    const action = document.querySelector<HTMLElement>('#search-action')!
    action.focus()
    escape(action)
    await waitFor(() => !document.querySelector('[role="listbox"]'))
    expect(document.querySelector('.modal-container.shown')).not.toBeNull()
    await waitFor(() => document.activeElement === trigger)
    escape(trigger)
    await waitFor(() => !document.querySelector('[role="dialog"]'))
})

it('closes the multiselect from its bottom checkbox without closing the modal', async () => {
    await modal(
        h(
            MultiSelect,
            {
                options: [{ value: 'all', label: 'All versions' }],
                modelValue: [],
            },
            { bottom: () => h('input', { type: 'checkbox', id: 'show-all' }) },
        ),
    )
    const trigger = document.querySelector<HTMLElement>('[aria-haspopup="listbox"]')!
    trigger.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }),
    )
    await waitFor(() => !!document.querySelector('#show-all'))
    const checkbox = document.querySelector<HTMLElement>('#show-all')!
    checkbox.focus()
    escape(checkbox)
    await waitFor(() => !document.querySelector('[role="listbox"]'))
    expect(document.querySelector('.modal-container.shown')).not.toBeNull()
    await waitFor(() => document.activeElement === trigger)
})

it('respects a nested control consuming Escape inside a dropdown', async () => {
    await modal(
        h(
            MultiSelect,
            {
                options: [{ value: 'all', label: 'All versions' }],
                modelValue: [],
            },
            {
                bottom: () =>
                    h(
                        'button',
                        {
                            id: 'nested-control',
                            onKeydown: (event: KeyboardEvent) => event.preventDefault(),
                        },
                        'Nested',
                    ),
            },
        ),
    )
    const trigger = document.querySelector<HTMLElement>('[aria-haspopup="listbox"]')!
    trigger.dispatchEvent(
        new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true, cancelable: true }),
    )
    await waitFor(() => !!document.querySelector('#nested-control'))
    escape(document.querySelector('#nested-control')!)
    expect(document.querySelector('[role="listbox"]')).not.toBeNull()
    expect(document.querySelector('.modal-container.shown')).not.toBeNull()
})

it('keeps the parent modal locked when a menu closes or unmounts', async () => {
    document.body.style.setProperty('overflow', 'scroll', 'important')
    cleanup.push(() => document.body.style.removeProperty('overflow'))
    const mounted = ref(true)
    const Child = defineComponent({
        setup: () => () =>
            mounted.value
                ? h(TeleportOverflowMenu, {
                      options: [{ id: 'Inspect', action: () => {} }],
                      label: 'More',
                  })
                : null,
    })
    const { vm } = await modal(h(Child))
    const trigger = document.querySelector<HTMLElement>('[aria-haspopup="menu"]')!
    trigger.click()
    await waitFor(() => !!document.querySelector('[data-pyro-telepopover-root]'))
    expect(document.body.style.overflow).toBe('hidden')
    document.querySelector<HTMLButtonElement>('[data-pyro-telepopover-root] button')!.click()
    await waitFor(() => !document.querySelector('[data-pyro-telepopover-root]'))
    expect(document.body.style.overflow).toBe('hidden')
    trigger.click()
    await waitFor(() => !!document.querySelector('[data-pyro-telepopover-root]'))
    mounted.value = false
    await nextTick()
    expect(document.body.style.overflow).toBe('hidden')
    await vm.hide()
    expect(document.body.style.overflow).toBe('scroll')
    expect(document.body.style.getPropertyPriority('overflow')).toBe('important')
})

it('releases only the current owner regardless of close order or repeated calls', () => {
    const first = effectScope()
    const second = effectScope()
    cleanup.push(() => {
        first.stop()
        second.stop()
    })
    const a = first.run(useBodyScrollLock)!
    const b = second.run(useBodyScrollLock)!
    const previous = document.body.style.overflow
    a.lock()
    a.lock()
    b.lock()
    a.unlock()
    a.unlock()
    expect(document.body.style.overflow).toBe('hidden')
    first.stop()
    expect(document.body.style.overflow).toBe('hidden')
    second.stop()
    expect(document.body.style.overflow).toBe(previous)
    b.lock()
    expect(document.body.style.overflow).toBe(previous)
})

it('restores independent overflow axes and their priorities', () => {
    document.body.style.setProperty('overflow-x', 'scroll', 'important')
    document.body.style.setProperty('overflow-y', 'auto')
    cleanup.push(() => document.body.style.removeProperty('overflow'))
    const scope = effectScope()
    const owner = scope.run(useBodyScrollLock)!
    cleanup.push(() => scope.stop())
    owner.lock()
    expect(document.body.style.overflowX).toBe('hidden')
    expect(document.body.style.overflowY).toBe('hidden')
    owner.unlock()
    expect(document.body.style.overflowX).toBe('scroll')
    expect(document.body.style.overflowY).toBe('auto')
    expect(document.body.style.getPropertyPriority('overflow-x')).toBe('important')
    expect(document.body.style.getPropertyPriority('overflow-y')).toBe('')
})
