// src/lib/utils/logger.ts
// (Using the one from your initial project is perfectly fine)

type LogConfig = {
	level: 'debug' | 'info' | 'warn' | 'error';
	enabled: boolean;
};

const config: LogConfig = {
	level: import.meta.env.DEV ? 'debug' : 'info', // Default to debug in dev, info in prod
	enabled: true // Always enabled, level controls output
};

const LOG_LEVELS = {
	debug: 0,
	info: 1,
	warn: 2,
	error: 3
};

function createLogger(type: 'log' | 'info' | 'warn' | 'error' | 'debug') {
	return (...args: unknown[]) => {
		if (!config.enabled || LOG_LEVELS[type] < LOG_LEVELS[config.level]) {
			return;
		}

		const timestamp = new Date().toISOString();
		const prefix = `[${timestamp}] [${type.toUpperCase()}]`;

		switch (type) {
			case 'debug':
				console.debug(prefix, ...args);
				break;
			case 'info':
				console.info(prefix, ...args);
				break;
			case 'warn':
				console.warn(prefix, ...args);
				break;
			case 'error':
				console.error(prefix, ...args);
				break;
			default:
				console.log(prefix, ...args);
		}
	};
}

export const log = createLogger('log');
export const info = createLogger('info');
export const warn = createLogger('warn');
export const error = createLogger('error');
export const debug = createLogger('debug');

export function enableLogging(enabled: boolean = true) {
	config.enabled = enabled;
}

export function setLogLevel(level: LogConfig['level']) {
	config.level = level;
}
