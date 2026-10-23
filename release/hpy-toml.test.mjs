import assert from 'node:assert/strict'
import { readdirSync, readFileSync, existsSync } from 'node:fs'
import { join } from 'node:path'
import test from 'node:test'

const root = join(import.meta.dirname, '..', 'plugins', 'happyview')

const files = readdirSync(root)
	.map((name) => join(root, name, 'hpy.toml'))
	.filter((path) => existsSync(path))

const sections = (text) =>
	[...text.matchAll(/^\s*\[([^\]]+)\]\s*(?:#.*)?$/gm)].map((match) => match[1].trim())

test('there are hpy.toml files to check', () => {
	assert.ok(files.length > 0)
})

for (const file of files) {
	test(`${file} uses [project] and [release], not [profile]`, () => {
		const found = sections(readFileSync(file, 'utf8'))
		assert.ok(found.includes('project'), 'missing [project]')
		assert.ok(found.includes('release'), 'missing [release]')
		assert.deepEqual(
			found.filter((name) => name === 'profile' || name.startsWith('profile.')),
			[],
			'still has [profile]',
		)
	})
}
