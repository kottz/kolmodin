import { PUBLIC_API_BASE_URL } from '$env/static/public';
import type { ApiErrorResponse } from '$lib/types/general.types';
import { warn, error as logError } from '$lib/utils/logger';

// Define API endpoints based on Rust server
export const API_ENDPOINTS = {
	// GET_AVAILABLE_GAMES: '/api/games', // Keep if you plan to add it, or remove/comment out
	CREATE_LOBBY: '/api/create-lobby' // Matches Rust server
};

interface RequestOptions extends RequestInit {
	// No custom options needed for now
}

// handleResponse function remains the same
async function handleResponse<T>(response: Response): Promise<T> {
	if (!response.ok) {
		let errorData: ApiErrorResponse;
		// Rust server sends plain string on error, not JSON for (StatusCode, String)
		const errorText = await response.text();
		errorData = {
			error: `HTTPError:${response.status}`,
			message: errorText || response.statusText || 'An unknown server error occurred.'
		};
		logError('API Error:', response.status, errorData.message);
		throw errorData;
	}
	const contentType = response.headers.get('content-type');
	if (response.status === 204) {
		// Handle No Content specifically
		return undefined as unknown as T;
	}
	if (contentType && contentType.includes('application/json')) {
		return response.json() as Promise<T>;
	}
	// If not JSON and not 204, it might be an issue or an unexpected response type
	warn('API response was not JSON:', await response.text());
	return undefined as unknown as T;
}

// baseFetch function remains the same
const baseFetch = async <T>(endpoint: string, options: RequestOptions = {}): Promise<T> => {
	const url = `${PUBLIC_API_BASE_URL}${endpoint}`;
	const defaultHeaders: HeadersInit = {
		'Content-Type': 'application/json',
		Accept: 'application/json' // Good practice to include Accept header
	};

	const config: RequestInit = {
		...options,
		headers: {
			...defaultHeaders,
			...options.headers
		}
	};

	const response = await fetch(url, config);
	return handleResponse<T>(response);
};

// apiClient object remains the same
export const apiClient = {
	get: <T>(endpoint: string, options?: RequestOptions) =>
		baseFetch<T>(endpoint, { ...options, method: 'GET' }),
	post: <T, U = any>(endpoint: string, body?: U, options?: RequestOptions) =>
		baseFetch<T>(endpoint, {
			...options,
			method: 'POST',
			body: body ? JSON.stringify(body) : undefined
		}),
	put: <T, U = any>(endpoint: string, body?: U, options?: RequestOptions) =>
		baseFetch<T>(endpoint, {
			...options,
			method: 'PUT',
			body: body ? JSON.stringify(body) : undefined
		}),
	delete: <T>(endpoint: string, options?: RequestOptions) =>
		baseFetch<T>(endpoint, { ...options, method: 'DELETE' })
};

// isApiError function remains the same
export function isApiError(error: unknown): error is ApiErrorResponse {
	return typeof error === 'object' && error !== null && 'error' in error && 'message' in error;
}
