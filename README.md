_Early development stage, look and feel is pretty bad but it is usable_

# Features
## Implemented
- *lag-free* visualisation of large json array: only visible rows and columns are rendered
- Select column to render
- Filter out rows with null value at given columns
- Scroll to column
- Pin columns to left
- Open nested array in sub-pannel
- Select depth for nested object

![](./github/json-editor.png)

Json in video below is 372mb with an array of 309_759 entries, containing nested objects, it runs at ~60fps



https://github.com/nmeylan/json-table-editor/assets/1909074/3e7deb79-96b3-4c9e-ba11-064949f27520


## Performance and Memory usage
This editor use a custom json parser to deserialize json into a flat data structure allowing O(1) data access. 
While it is not the fastest parser on the market, it is still faster to use this custom one instead of parsing using serde or other library generating tree data structure and then convert it to flat data structure. 

As this structure is flat, it also allows to parse only a subset of json files by defining a depth limit, the json files is still fully read but after a given depth, 
content is not deserialize we only keep raw content as String which then can be parse later.

This mecanism allow fast parsing of big json files, but consume more memory as for each depth level we store the full string and the parsed content.
Additionally, this mecanism allow to serialize only row that have been changed, unchanged rows are already serialized.

## Todo
- Cell edition
- Serialization
- File selection
- Add new column
- Add new row
