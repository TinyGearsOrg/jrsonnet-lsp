// A small library imported by main.jsonnet.
{
  greet(name):: 'Hello, ' + name + '!',
  config: { name: 'world', verbose: false },
}
