from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.block_response import BlockResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    *,
    limit: int | Unset = UNSET,
    before_height: int | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["before_height"] = before_height


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/blocks",
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> list[BlockResponse] | None:
    if response.status_code == 200:
        response_200 = []
        _response_200 = response.json()
        for response_200_item_data in (_response_200):
            response_200_item = BlockResponse.from_dict(response_200_item_data)



            response_200.append(response_200_item)

        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[list[BlockResponse]]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before_height: int | Unset = UNSET,

) -> Response[list[BlockResponse]]:
    """ GET /v1/blocks?limit=N&before_height=H — blocks newest-first, paged by height.

    Args:
        limit (int | Unset):
        before_height (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[list[BlockResponse]]
     """


    kwargs = _get_kwargs(
        limit=limit,
before_height=before_height,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before_height: int | Unset = UNSET,

) -> list[BlockResponse] | None:
    """ GET /v1/blocks?limit=N&before_height=H — blocks newest-first, paged by height.

    Args:
        limit (int | Unset):
        before_height (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        list[BlockResponse]
     """


    return sync_detailed(
        client=client,
limit=limit,
before_height=before_height,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before_height: int | Unset = UNSET,

) -> Response[list[BlockResponse]]:
    """ GET /v1/blocks?limit=N&before_height=H — blocks newest-first, paged by height.

    Args:
        limit (int | Unset):
        before_height (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[list[BlockResponse]]
     """


    kwargs = _get_kwargs(
        limit=limit,
before_height=before_height,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    before_height: int | Unset = UNSET,

) -> list[BlockResponse] | None:
    """ GET /v1/blocks?limit=N&before_height=H — blocks newest-first, paged by height.

    Args:
        limit (int | Unset):
        before_height (int | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        list[BlockResponse]
     """


    return (await asyncio_detailed(
        client=client,
limit=limit,
before_height=before_height,

    )).parsed
