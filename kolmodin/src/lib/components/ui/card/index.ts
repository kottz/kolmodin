import Root from './Card.svelte';
import Header from './CardHeader.svelte';
import Content from './CardContent.svelte';
import Footer from './CardFooter.svelte';
import Title from './CardTitle.svelte';
import Description from './CardDescription.svelte';

// For CardTitle, we might allow specifying the heading level
export type HeadingLevel = 'h1' | 'h2' | 'h3' | 'h4' | 'h5' | 'h6';

export {
	Root,
	Header,
	Content,
	Footer,
	Title,
	Description,
	// Aliases for convenience
	Root as Card,
	Header as CardHeader,
	Content as CardContent,
	Footer as CardFooter,
	Title as CardTitle,
	Description as CardDescription,
	type HeadingLevel
};
