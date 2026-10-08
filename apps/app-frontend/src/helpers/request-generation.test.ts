import assert from 'node:assert/strict'
import test from 'node:test'

import { createRequestGeneration } from './request-generation.ts'

test('only the latest request for the current key can publish', () => {
    const requests = createRequestGeneration()
    const first = requests.begin('instance-a:/')
    const second = requests.begin('instance-a:mods')

    assert.equal(requests.isCurrent(first, 'instance-a:/'), false)
    assert.equal(requests.isCurrent(second, 'instance-a:mods'), true)
    assert.equal(requests.isCurrent(second, 'instance-a:/'), false)
})

test('invalidating a workspace makes an in-flight request stale', () => {
    const requests = createRequestGeneration()
    const token = requests.begin('server-a:/')
    requests.invalidate()

    assert.equal(requests.isCurrent(token, 'server-a:/'), false)
})
