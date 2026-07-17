from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.api_error_response import ApiErrorResponse
from ...models.public_block_response import PublicBlockResponse
from typing import cast



def _get_kwargs(
    height: int,

) -> dict[str, Any]:
    

    

    

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/blocks/{height}".format(height=quote(str(height), safe=""),),
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Any | ApiErrorResponse | PublicBlockResponse | None:
    if response.status_code == 200:
        response_200 = PublicBlockResponse.from_dict(response.json())



        return response_200

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if response.status_code == 410:
        response_410 = ApiErrorResponse.from_dict(response.json())



        return response_410

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[Any | ApiErrorResponse | PublicBlockResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse | PublicBlockResponse]:
    """ GET /v1/blocks/{height}

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | PublicBlockResponse]
     """


    kwargs = _get_kwargs(
        height=height,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | PublicBlockResponse | None:
    """ GET /v1/blocks/{height}

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | PublicBlockResponse
     """


    return sync_detailed(
        height=height,
client=client,

    ).parsed

async def asyncio_detailed(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Response[Any | ApiErrorResponse | PublicBlockResponse]:
    """ GET /v1/blocks/{height}

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ApiErrorResponse | PublicBlockResponse]
     """


    kwargs = _get_kwargs(
        height=height,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    height: int,
    *,
    client: AuthenticatedClient | Client,

) -> Any | ApiErrorResponse | PublicBlockResponse | None:
    """ GET /v1/blocks/{height}

    Args:
        height (int):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ApiErrorResponse | PublicBlockResponse
     """


    return (await asyncio_detailed(
        height=height,
client=client,

    )).parsed
