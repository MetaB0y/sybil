from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.market_response import MarketResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    *,
    q: str | Unset = UNSET,
    tags: str | Unset = UNSET,
    category: str | Unset = UNSET,
    status: str | Unset = UNSET,
    min_volume: int | Unset = UNSET,
    sort: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["q"] = q

    params["tags"] = tags

    params["category"] = category

    params["status"] = status

    params["min_volume"] = min_volume

    params["sort"] = sort

    params["limit"] = limit

    params["offset"] = offset


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/markets/search",
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> list[MarketResponse] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in (_response_200):
            response_200_item = MarketResponse.from_dict(response_200_item_data)



            response_200.append(response_200_item)

        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[list[MarketResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    q: str | Unset = UNSET,
    tags: str | Unset = UNSET,
    category: str | Unset = UNSET,
    status: str | Unset = UNSET,
    min_volume: int | Unset = UNSET,
    sort: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,

) -> Response[list[MarketResponse]]:
    """ GET /v1/markets/search

    Args:
        q (str | Unset):
        tags (str | Unset):
        category (str | Unset):
        status (str | Unset):
        min_volume (int | Unset):
        sort (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[list[MarketResponse]]
     """


    kwargs = _get_kwargs(
        q=q,
tags=tags,
category=category,
status=status,
min_volume=min_volume,
sort=sort,
limit=limit,
offset=offset,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    q: str | Unset = UNSET,
    tags: str | Unset = UNSET,
    category: str | Unset = UNSET,
    status: str | Unset = UNSET,
    min_volume: int | Unset = UNSET,
    sort: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,

) -> list[MarketResponse] | None:
    """ GET /v1/markets/search

    Args:
        q (str | Unset):
        tags (str | Unset):
        category (str | Unset):
        status (str | Unset):
        min_volume (int | Unset):
        sort (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        list[MarketResponse]
     """


    return sync_detailed(
        client=client,
q=q,
tags=tags,
category=category,
status=status,
min_volume=min_volume,
sort=sort,
limit=limit,
offset=offset,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    q: str | Unset = UNSET,
    tags: str | Unset = UNSET,
    category: str | Unset = UNSET,
    status: str | Unset = UNSET,
    min_volume: int | Unset = UNSET,
    sort: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,

) -> Response[list[MarketResponse]]:
    """ GET /v1/markets/search

    Args:
        q (str | Unset):
        tags (str | Unset):
        category (str | Unset):
        status (str | Unset):
        min_volume (int | Unset):
        sort (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[list[MarketResponse]]
     """


    kwargs = _get_kwargs(
        q=q,
tags=tags,
category=category,
status=status,
min_volume=min_volume,
sort=sort,
limit=limit,
offset=offset,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    q: str | Unset = UNSET,
    tags: str | Unset = UNSET,
    category: str | Unset = UNSET,
    status: str | Unset = UNSET,
    min_volume: int | Unset = UNSET,
    sort: str | Unset = UNSET,
    limit: int | Unset = UNSET,
    offset: int | Unset = UNSET,

) -> list[MarketResponse] | None:
    """ GET /v1/markets/search

    Args:
        q (str | Unset):
        tags (str | Unset):
        category (str | Unset):
        status (str | Unset):
        min_volume (int | Unset):
        sort (str | Unset):
        limit (int | Unset):
        offset (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        list[MarketResponse]
     """


    return (await asyncio_detailed(
        client=client,
q=q,
tags=tags,
category=category,
status=status,
min_volume=min_volume,
sort=sort,
limit=limit,
offset=offset,

    )).parsed
